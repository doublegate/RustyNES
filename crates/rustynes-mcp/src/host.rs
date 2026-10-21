//! The server host: the session behind a lock, the real-time runner, and the
//! MCP wire (the official `rmcp` SDK, stdio transport).
//!
//! # Pause and resume
//!
//! The machine is **paused at start-up**, so an agent plays turn by turn by
//! default: observe, decide, `step_frames`. `resume` starts a runner thread
//! that runs the main machine at its region's frame rate with the held input
//! (`set_input` changes it between frames), recording each frame in the
//! journal like any other; `pause` stops it and reports how far it ran. The
//! runner also stops at an exec breakpoint, at a CPU jam, and after
//! `max_frames` (at most [`crate::limits::MAX_REALTIME_FRAMES`]), so an agent
//! that forgets to pause does not leave a machine running for ever.
//!
//! The runner takes the session lock once per frame and sleeps outside it, so
//! observation tools answer while the machine runs. Stepping tools, `reset`
//! and `fork_adopt` are refused until it is paused: two writers on one
//! timeline would make the journal describe neither.
//!
//! # Blocking work
//!
//! A tool call can run for seconds (a 36,000-frame `fork_run`). Each call runs
//! on tokio's blocking pool, so the transport keeps reading (pings,
//! cancellations) while the core works.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::Instant;

use base64::Engine as _;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    InitializeResult, ListToolsResult, PaginatedRequestParams, ServerCapabilities, Tool,
};
use rmcp::service::{MaybeSendFuture, RequestContext};
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt};
use serde_json::{Value, json};

use crate::limits;
use crate::session::{Session, ToolError, ToolOutput};
use crate::tools::{self, ResumeArgs};

struct Inner {
    session: Mutex<Session>,
    running: AtomicBool,
    runner: Mutex<Option<JoinHandle<()>>>,
    /// Frames the current (or last) real-time run has completed.
    rt_frames: AtomicU64,
    /// Why the last real-time run stopped.
    rt_stop: Mutex<String>,
}

/// The session plus its runner. Cheap to clone (an `Arc`).
#[derive(Clone)]
pub struct Host {
    inner: Arc<Inner>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    // A panic in a tool must not wedge every later call; the session's own
    // invariants hold between calls, so the poisoned value is still usable.
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Host {
    /// Wrap a session.
    #[must_use]
    pub fn new(session: Session) -> Self {
        Self {
            inner: Arc::new(Inner {
                session: Mutex::new(session),
                running: AtomicBool::new(false),
                runner: Mutex::new(None),
                rt_frames: AtomicU64::new(0),
                rt_stop: Mutex::new(String::new()),
            }),
        }
    }

    /// Run `f` with the session locked (tests and the binary's status line).
    pub fn with_session<R>(&self, f: impl FnOnce(&mut Session) -> R) -> R {
        f(&mut lock(&self.inner.session))
    }

    /// Dispatch one tool call: `pause` / `resume` here, everything else in
    /// [`Session::call`].
    ///
    /// # Errors
    ///
    /// See [`ToolError`].
    pub fn call(
        &self,
        name: &str,
        args: Option<&serde_json::Map<String, Value>>,
    ) -> Result<ToolOutput, ToolError> {
        match name {
            "pause" => {
                let v = Value::Object(args.cloned().unwrap_or_default());
                serde_json::from_value::<tools::NoArgs>(v)
                    .map_err(|e| ToolError::InvalidArgs(e.to_string()))?;
                Ok(self.pause())
            }
            "resume" => {
                let v = Value::Object(args.cloned().unwrap_or_default());
                let a: ResumeArgs =
                    serde_json::from_value(v).map_err(|e| ToolError::InvalidArgs(e.to_string()))?;
                self.resume(a.max_frames)
            }
            _ => lock(&self.inner.session).call(name, args),
        }
    }

    fn join_runner(&self) {
        let handle = lock(&self.inner.runner).take();
        if let Some(h) = handle {
            let _ = h.join();
        }
    }

    fn pause(&self) -> ToolOutput {
        let was_running = self.inner.running.swap(false, Ordering::SeqCst);
        self.join_runner();
        lock(&self.inner.session).set_running(false);
        if was_running {
            "paused".clone_into(&mut lock(&self.inner.rt_stop));
        }
        let s = lock(&self.inner.session);
        ToolOutput {
            json: json!({
                "running": false,
                "was_running": was_running,
                "realtime_frames": self.inner.rt_frames.load(Ordering::SeqCst),
                "last_stop": *lock(&self.inner.rt_stop),
                "frame": s.nes().frame(),
            }),
            ..ToolOutput::default()
        }
    }

    fn resume(&self, max_frames: Option<u32>) -> Result<ToolOutput, ToolError> {
        let max = max_frames.unwrap_or(limits::MAX_REALTIME_FRAMES);
        if max == 0 || max > limits::MAX_REALTIME_FRAMES {
            return Err(ToolError::InvalidArgs(format!(
                "max_frames is 1-{}",
                limits::MAX_REALTIME_FRAMES
            )));
        }
        if self.inner.running.load(Ordering::SeqCst) {
            return Err(ToolError::Refused("already running".into()));
        }
        // A run that ended by itself (breakpoint, jam, max_frames) left its
        // finished thread behind.
        self.join_runner();
        let frame_time = {
            let mut s = lock(&self.inner.session);
            s.set_running(true);
            s.nes().frame_duration()
        };
        self.inner.rt_frames.store(0, Ordering::SeqCst);
        lock(&self.inner.rt_stop).clear();
        self.inner.running.store(true, Ordering::SeqCst);
        let inner = Arc::clone(&self.inner);
        let handle = std::thread::Builder::new()
            .name("mcp-realtime".into())
            .spawn(move || {
                let mut next = Instant::now();
                let mut stop = format!("ran max_frames ({max})");
                for _ in 0..max {
                    if !inner.running.load(Ordering::SeqCst) {
                        stop = "paused".into();
                        break;
                    }
                    let (hit, jammed) = {
                        let mut s = lock(&inner.session);
                        let hit = s.run_main_frame();
                        (hit, s.nes().cpu().is_jammed())
                    };
                    inner.rt_frames.fetch_add(1, Ordering::SeqCst);
                    if let Some(pc) = hit {
                        stop = format!("breakpoint ${pc:04X}");
                        break;
                    }
                    if jammed {
                        stop = "CPU jammed".into();
                        break;
                    }
                    next += frame_time;
                    let now = Instant::now();
                    if next > now {
                        std::thread::sleep(next - now);
                    } else {
                        // Fell behind (a slow host or a long-held lock):
                        // run on rather than burst to catch up.
                        next = now;
                    }
                }
                inner.running.store(false, Ordering::SeqCst);
                lock(&inner.session).set_running(false);
                *lock(&inner.rt_stop) = stop;
            })
            .map_err(|e| ToolError::Failed(format!("runner thread: {e}")))?;
        *lock(&self.inner.runner) = Some(handle);
        Ok(ToolOutput {
            json: json!({ "running": true, "max_frames": max }),
            ..ToolOutput::default()
        })
    }

    /// Whether the real-time runner is running.
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.inner.running.load(Ordering::SeqCst)
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
    }
}

/// Render a [`ToolOutput`] as MCP content: the JSON first (also as
/// `structuredContent`), then each text block, then the image.
#[must_use]
pub fn to_call_result(out: ToolOutput) -> CallToolResult {
    let mut content = vec![ContentBlock::text(
        serde_json::to_string_pretty(&out.json).unwrap_or_default(),
    )];
    content.extend(out.texts.into_iter().map(ContentBlock::text));
    if let Some(png) = out.png {
        content.push(ContentBlock::image(
            base64::engine::general_purpose::STANDARD.encode(png),
            "image/png",
        ));
    }
    let mut r = CallToolResult::success(content);
    if out.json.is_object() {
        r.structured_content = Some(out.json);
    }
    r
}

/// The MCP server: [`Host`] behind `rmcp`'s [`ServerHandler`].
#[derive(Clone)]
pub struct McpServer {
    host: Host,
}

impl McpServer {
    /// Serve `host`.
    #[must_use]
    pub const fn new(host: Host) -> Self {
        Self { host }
    }
}

const INSTRUCTIONS: &str = "RustyNES headless NES emulator. The machine starts paused: observe \
(observe, screenshot, nametable, oam, cpu_state, read_memory), act (set_input, step_frames), and \
run experiments with fork_create -> fork_run(inputs) -> fork_diff; movie_verify replays any fork \
from its recorded inputs. Lua (lua_eval) is read-only.";

impl ServerHandler for McpServer {
    fn get_info(&self) -> InitializeResult {
        InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                "rustynes-mcp",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(INSTRUCTIONS)
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListToolsResult, ErrorData>> + MaybeSendFuture + '_ {
        std::future::ready(Ok(ListToolsResult::with_all_items(tools::catalogue())))
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        tools::catalogue().into_iter().find(|t| t.name == name)
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let host = self.host.clone();
        let name = request.name.to_string();
        let args = request.arguments;
        let result = tokio::task::spawn_blocking(move || host.call(&name, args.as_ref()))
            .await
            .map_err(|e| ErrorData::internal_error(format!("tool task: {e}"), None))?;
        match result {
            Ok(out) => Ok(to_call_result(out).into()),
            // An undeclared tool is a protocol error (JSON-RPC -32602), not a
            // tool failure: there is no such tool to have failed.
            Err(e @ ToolError::UnknownTool(_)) => {
                Err(ErrorData::invalid_params(e.to_string(), None))
            }
            Err(e) => Ok(CallToolResult::error(vec![ContentBlock::text(e.to_string())]).into()),
        }
    }
}

/// Serve `host` over stdin/stdout until the client disconnects.
///
/// # Errors
///
/// A transport or initialisation failure.
pub async fn serve_stdio(host: Host) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let service = McpServer::new(host.clone())
        .serve(rmcp::transport::stdio())
        .await?;
    service.waiting().await?;
    // The client is gone: stop a real-time run rather than leave it going.
    let _ = host.call("pause", None);
    Ok(())
}
