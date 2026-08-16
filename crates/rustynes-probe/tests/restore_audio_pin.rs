//! Pins an assumption the probe engine depends on, raised in review on #384.
//!
//! A reviewer flagged possible **cross-trial audio contamination**: `sample`
//! drained audio only for the `AudioEnergy` observable, so if `Nes::restore` kept
//! the pending presentation queue — as save-states in many emulators do — the
//! undrained framebuffer trials would pile up hundreds of frames, and the first
//! `AudioEnergy` trial would drain them all on frame 0 and diverge falsely
//! against every later trial.
//!
//! It does not happen here: `restore` replaces the blip wholesale and drops the
//! pending queue, which `rustynes-apu`'s snapshot module documents deliberately.
//! This test is the evidence for that claim rather than the claim itself —
//! measured, not read off a comment. The engine now also drains unconditionally,
//! so the property is belt-and-braces, but if `restore` ever starts preserving
//! audio this test says so directly instead of the failure surfacing as a
//! mysterious "every game has zero input lag".

#[test]
fn restore_drops_pending_audio_so_trials_cannot_contaminate_each_other() {
    use rustynes_core::Nes;
    let mut prg = vec![0u8; 16 * 1024];
    prg[0] = 0x4C;
    prg[1] = 0x00;
    prg[2] = 0xC0;
    let len = prg.len();
    for (i, b) in [0x00u8, 0xC0, 0x00, 0xC0, 0x00, 0xC0].iter().enumerate() {
        prg[len - 6 + i] = *b;
    }
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"NES\x1A");
    bytes.extend_from_slice(&[1, 1, 0, 0]);
    bytes.extend_from_slice(&[0u8; 8]);
    bytes.extend_from_slice(&prg);
    bytes.extend_from_slice(&vec![0u8; 8 * 1024]);

    let mut nes = Nes::from_rom(&bytes).unwrap();
    let snap = nes.snapshot();
    // Accumulate WITHOUT draining, exactly as a Framebuffer-observable trial does.
    for _ in 0..30 {
        nes.run_frame();
    }
    let accumulated = nes.drain_audio().len();
    assert!(
        accumulated > 10_000,
        "premise: 30 undrained frames accumulate (got {accumulated})"
    );

    // Re-accumulate, then restore and drain: if restore keeps the queue, this
    // matches `accumulated`; if it drops it, this is ~one frame's worth.
    for _ in 0..30 {
        nes.run_frame();
    }
    nes.restore(&snap).unwrap();
    nes.run_frame();
    let after_restore = nes.drain_audio().len();
    println!("accumulated={accumulated} after_restore={after_restore}");
    assert!(
        after_restore < accumulated / 10,
        "restore did NOT drop pending audio: {after_restore} vs {accumulated} — \
         cross-trial audio contamination is real"
    );
}
