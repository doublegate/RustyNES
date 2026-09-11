using TriCNES;
class Program {
    static void Main(string[] args) {
        var emu = new Emulator();
        // ROM path: env TRICNES_ROM overrides; otherwise resolve the canonical
        // repo copy RELATIVE to the working directory.
        //
        // This used to hard-code an absolute path into a workspace layout that
        // no longer exists (`Commercial_Private-Projects/RustyNES_v2/...`,
        // pre-reorg). An absolute default silently rots the moment the repo
        // moves, and it fails as "file not found" rather than as "your default
        // is stale", so say so explicitly instead.
        var romPath = System.Environment.GetEnvironmentVariable("TRICNES_ROM")
            ?? System.IO.Path.Combine(
                System.IO.Directory.GetCurrentDirectory(),
                "tests", "roms", "accuracycoin", "AccuracyCoin.nes");
        if (!System.IO.File.Exists(romPath))
        {
            System.Console.Error.WriteLine(
                $"tricnes-harness: ROM not found at {romPath}\n" +
                "  Run from the repository root, or set TRICNES_ROM to the ROM path.");
            System.Environment.Exit(2);
        }
        var cart = new Cartridge(romPath);
        emu.Cart = cart;
        cart.Emu = emu;
        // xdiff cross-diff window (per-cycle bus log). Set XDIFF_LO/XDIFF_HI to
        // a HarnessCycle range to emit "XC ..." rows there; leave unset (pass 1)
        // to only emit the "W54A ..." anchor markers. XDIFF_ANCHOR (hex)
        // overrides the anchor addr (default 054A = Implicit Loop3 X=10 store).
        {
            var lo = System.Environment.GetEnvironmentVariable("XDIFF_LO");
            var hi = System.Environment.GetEnvironmentVariable("XDIFF_HI");
            var an = System.Environment.GetEnvironmentVariable("XDIFF_ANCHOR");
            if (lo != null) emu.XdiffLo = long.Parse(lo);
            if (hi != null) emu.XdiffHi = long.Parse(hi);
            if (an != null) emu.XdiffAnchorAddr = System.Convert.ToUInt16(an, 16);
        }
        int frames = args.Length > 0 ? int.Parse(args[0]) : 2000;
        for (int i = 0; i < 300; i++) emu._CoreFrameAdvance();   // boot/menu
        emu.ControllerPort1 = 0x10;                               // press START
        for (int i = 0; i < 6; i++) emu._CoreFrameAdvance();
        emu.ControllerPort1 = 0x00;                               // release
        for (int i = 0; i < frames; i++) emu._CoreFrameAdvance(); // run battery
        System.Console.Error.WriteLine($"# done {frames+306} frames, HarnessCycle={emu.HarnessCycle}");
    }
}
