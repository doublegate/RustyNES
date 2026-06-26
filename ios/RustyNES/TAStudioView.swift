//
//  TAStudioView.swift
//
//  A pragmatic touch TAStudio piano-roll (v1.9.9 "Workshop"). The user authors a
//  host-side table of per-frame P1 inputs (8 button columns); "Play" injects the
//  table one mask per frame through the EXISTING bridge (deterministic input via
//  setButtons + runFrame), and "Save as .rnm" arms the core's recorder and replays
//  the table so the captured input is written to a real native `.rnm` movie.
//
//  This stays within the additive mobile bridge surface: it adds no movie-editing
//  API to the core. It does NOT pause emulation (playback needs the frame loop
//  running), mirroring the Movies panel.
//

import Combine
import SwiftUI

struct TAStudioView: View {
    @EnvironmentObject private var model: AppModel
    @Environment(\.dismiss) private var dismiss

    /// One NES button column: a display glyph + its mask bit (matching the bridge's
    /// `Buttons` order: A, B, Select, Start, Up, Down, Left, Right).
    private struct Btn: Identifiable {
        let glyph: String
        let mask: UInt8
        var id: UInt8 { mask }
    }
    private let buttons: [Btn] = [
        Btn(glyph: "A", mask: 0x01), Btn(glyph: "B", mask: 0x02),
        Btn(glyph: "Se", mask: 0x04), Btn(glyph: "St", mask: 0x08),
        Btn(glyph: "\u{2191}", mask: 0x10), Btn(glyph: "\u{2193}", mask: 0x20),
        Btn(glyph: "\u{2190}", mask: 0x40), Btn(glyph: "\u{2192}", mask: 0x80),
    ]

    /// The per-frame P1 masks being authored.
    @State private var frames: [UInt8] = Array(repeating: 0, count: 30)
    @State private var exporting = false

    /// Polls for scripted-export completion when exporting to `.rnm`. This is a
    /// connectable (NOT autoconnected) publisher: it is connected ONLY while an export
    /// is in flight — `startExport` connects it, `finishExport` (and a vanished core)
    /// cancel it — so it consumes no CPU when the panel is idle.
    private let pollTimer = Timer.publish(every: 0.2, on: .main, in: .common)
    @State private var pollConnection: Cancellable?

    var body: some View {
        NavigationStack {
            VStack(spacing: 0) {
                controlBar
                Divider()
                header
                pianoRoll
            }
            .navigationTitle("TAStudio")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") { dismiss() }
                }
            }
            .onReceive(pollTimer) { _ in
                // The timer only fires while connected (during an export); the guard
                // is a belt-and-suspenders no-op otherwise. Save once the core hands
                // back the finished movie. The core stops recording at the exact last
                // authored frame, so the saved `.rnm` carries no trailing idle frames.
                guard exporting else { return }
                if let bytes = model.emulator?.tasTakeExportedMovie() {
                    finishExport(bytes)
                } else if model.emulator == nil {
                    stopPolling()
                    exporting = false
                }
            }
            .onDisappear { stopPolling() }
        }
    }

    // MARK: - Controls

    private var controlBar: some View {
        VStack(spacing: 8) {
            HStack {
                Button {
                    frames.append(contentsOf: Array(repeating: 0, count: 30))
                } label: {
                    Label("Add 30", systemImage: "plus")
                }
                .buttonStyle(.bordered)
                Button(role: .destructive) {
                    frames = Array(repeating: 0, count: 30)
                } label: {
                    Label("Clear", systemImage: "trash")
                }
                .buttonStyle(.bordered)
                Spacer()
                Text(String(format: String(localized: "%lld frames"), frames.count))
                    .font(.caption.monospacedDigit())
                    .foregroundStyle(.secondary)
            }
            HStack {
                Button {
                    model.emulator?.tasStartPlayback(p1Masks: frames)
                } label: {
                    Label("Play", systemImage: "play.fill")
                }
                .buttonStyle(.borderedProminent)
                .disabled(model.emulator == nil || frames.isEmpty)

                Button {
                    model.emulator?.tasStop()
                } label: {
                    Label("Stop", systemImage: "stop.fill")
                }
                .buttonStyle(.bordered)

                Spacer()

                Button {
                    startExport()
                } label: {
                    Label("Save .rnm", systemImage: "square.and.arrow.down")
                }
                .buttonStyle(.bordered)
                .disabled(model.emulator == nil || frames.isEmpty || exporting)
            }
            // Saving records from power-on, so it restarts the running game (Play
            // does not). Surfaced so the restart is not a surprise.
            Text("Saving restarts the game to record from power-on.")
                .font(.caption2)
                .foregroundStyle(.secondary)
            if exporting {
                Label("Recording the playback to a movie\u{2026}", systemImage: "record.circle")
                    .font(.caption)
                    .foregroundStyle(.red)
            }
        }
        .padding()
    }

    private var header: some View {
        HStack(spacing: 4) {
            Text("Frame")
                .font(.caption2.bold())
                .frame(width: 56, alignment: .leading)
            ForEach(buttons) { b in
                Text(b.glyph)
                    .font(.caption2.bold())
                    .frame(maxWidth: .infinity)
            }
        }
        .padding(.horizontal)
        .padding(.vertical, 4)
        .foregroundStyle(.secondary)
    }

    private var pianoRoll: some View {
        ScrollView {
            LazyVStack(spacing: 2) {
                ForEach(frames.indices, id: \.self) { i in
                    frameRow(i)
                }
            }
            .padding(.horizontal)
            .padding(.bottom, 8)
        }
    }

    private func frameRow(_ i: Int) -> some View {
        HStack(spacing: 4) {
            Text("\(i)")
                .font(.caption2.monospacedDigit())
                .foregroundStyle(.secondary)
                .frame(width: 56, alignment: .leading)
            ForEach(buttons) { b in
                let on = (frames[i] & b.mask) != 0
                Button {
                    frames[i] ^= b.mask
                } label: {
                    Text(on ? b.glyph : "")
                        .font(.caption2.bold())
                        .frame(maxWidth: .infinity, minHeight: 28)
                        .background(on ? Color.accentColor.opacity(0.7) : Color.secondary.opacity(0.12))
                        .clipShape(RoundedRectangle(cornerRadius: 5))
                }
                .buttonStyle(.plain)
                .accessibilityLabel(
                    Text(String(format: String(localized: "Frame %lld, %@"), i, b.glyph))
                )
                .accessibilityValue(on ? Text("On") : Text("Off"))
            }
        }
    }

    // MARK: - Export to .rnm

    /// Arm the core's recorder from power-on and replay the authored table as one
    /// export: the core stops recording at the exact last authored frame and hands
    /// the finished bytes back via `tasTakeExportedMovie()`, which the poll timer
    /// drains into `finishExport`.
    private func startExport() {
        guard let e = model.emulator, !frames.isEmpty else { return }
        e.tasStartExport(p1Masks: frames)
        exporting = true
        // Connect the poll timer only now that an export is actually in flight.
        pollConnection = pollTimer.connect()
    }

    private func finishExport(_ bytes: Data) {
        exporting = false
        stopPolling()
        guard !bytes.isEmpty else { return }
        let name = (model.currentEntry?.name ?? "tas") + "-tas"
        try? model.movies.save(bytes, gameName: name)
    }

    /// Tear down the poll-timer connection so it stops firing when no export is
    /// pending. Idempotent.
    private func stopPolling() {
        pollConnection?.cancel()
        pollConnection = nil
    }
}
