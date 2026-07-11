# Header-excluded <rom crc32> values resolved from nes20db.xml for the 45
# cheat-DB game names that failed the automatic name-join (RustyNES GG re-key v2.1.3).
#
# Policy: include all Licensed/Unlicensed released region+revision+localization
# variants whose title is the SAME game (identical English title, or the sole
# release for Japan-only games). Excluded: Bootleg Singles (copier dumps),
# Homebrew/hacks/translations, Modern/Virtual Console, PlayChoice, Vs. System,
# Multicarts, Bad Dumps, Compatibility Hacks, Unreleased prototypes, and
# Japanese-RENAMED originals whose ROM differs from the Western dump the GG
# codes target (Kinnikuman, Hoshi no Kirby, Faria 封印の剣, Ultima III 恐怖のExodus).

ALIAS_CRCS = {
    "3-D WorldRunner": ["E6A477B2"],                                              # "The 3-D Battles of World Runner" (NA)
    "Advanced Dungeons _ Dragons - DragonStrike": ["2C5908A7"],
    "Advanced Dungeons _ Dragons - Heroes of the Lance": ["B17574F3", "1E472E7A"],  # NA + Japan (same English title)
    "Advanced Dungeons _ Dragons - Hillsfar": ["5DE61639", "2C33161D"],           # NA + Japan
    "Advanced Dungeons _ Dragons - Pool of Radiance": ["25952141", "CA730971"],   # NA + Japan
    "Adventure Island 3": ["BFBFD25D"],                                           # "Hudson's Adventure Island III" (NA)
    "Akumajou Densetsu": ["E349AF38"],                                            # 悪魔城伝説 (Japan-only)
    "Akumajou Special - Boku Dracula-kun": ["C1FBF659"],                          # 悪魔城 Special꞉ ぼく Dracula君 (Japan-only)
    "Archon": ["F304F1B9"],                                                       # "Archon꞉ The Light and the Dark" (NA)
    "Back to the Future Part II _ III": ["37BA3261"],
    "Battletoads-Double Dragon": ["CEB65B06", "23D7D48F"],                        # NA + PAL
    "Bill _ Ted's Excellent Video Game Adventure": ["C4B6ED3C"],
    "David Crane's A Boy and His Blob - Trouble on Blobolonia": ["4D1AC58C", "AB2AC325", "8ECBC577"],  # NA + PAL rev0/rev1
    "Dirty Harry": ["0C2E7863"],                                                  # "Dirty Harry꞉ The War Against Drugs"
    "Faria - A World of Mystery _ Danger!": ["45F03D2E"],                         # NA (Japan is renamed 封印の剣, excluded)
    "Flintstones, The - The Rescue of Dino _ Hoppy": ["2FE20D79", "40C0AD47", "AC609320"],  # NA + Japan + PAL
    "Fox's Peter Pan _ the Pirates - The Revenge of Captain Hook": ["20353E63"],  # "Peter Pan & The Pirates" (NA)
    "Gyromite": ["023A5A32"],                                                     # nes20db "Gyro Set" (R.O.B. Gyro, exp type 31) = Gyromite
    "IronSword - Wizards _ Warriors II": ["2328046E", "694C801F"],                # NA + PAL
    "Kirby's Adventure": ["D7794AFC", "5ED6F221", "37088EFF", "2C088DC5", "B2EF7F4B", "127D76F4"],  # NA rev0/rev1/Fr + PAL Eng/Fr/Ger
    "Krusty's Fun House": ["A0DF4B8F", "585BA83D"],                               # "The Simpsons꞉ Krusty's Fun House" NA + PAL
    "M.U.S.C.L.E. - Tag Team Match": ["8FF31896"],                               # "Tag Team Match꞉ M.U.S.C.L.E." (Japan is renamed Kinnikuman, excluded)
    "Mafat Conspiracy, The": ["8A043CD6"],                                        # "Golgo 13꞉ The Mafat Conspiracy" (NA)
    "Metal Mech - Man _ Machine": ["05378607"],                                   # "Metal Mech꞉ Man & Machine"
    "Muppet Adventure - Chaos at the Carnival": ["7156CB4D"],                     # "Jim Henson's Muppet Adventure꞉ Chaos at the Carnival"
    "North and South": ["AE9F33D0", "0FC8E9B7", "7BA3F8AE"],                      # NA + Japan + PAL
    "Pirates!": ["3D0996B2", "441DE6D8", "574E5F8B"],                             # "Sid Meier's Pirates!" NA + PAL Eng/Ger
    "Ren _ Stimpy Show, The - Buckeroo$!": ["E98AB943"],                          # "The Ren & Stimpy Show꞉ Buckaroo$!"
    "Skull _ Crossbones": ["B422A67A", "EC3B7B47"],                               # Unlicensed NA + South Korea
    "Snow Brothers": ["1DAC6208", "AAF49344", "A9660690"],                        # "Snow Bros." NA + Japan + PAL
    "Splatter House - Wanpaku Graffiti": ["46FD7843"],                            # "Splatterhouse꞉ わんぱくGraffiti" (Japan-only)
    "Star Wars": ["C1C3636B", "FCD772EB"],                                        # JVC "Star Wars꞉ A New Hope" NA + PAL (NOT the Japan Namco game, NOT Empire)
    "Street Fighter 2010 - The Final Fight": ["8DA651D4", "18A885B0"],            # NA + Japan (same title "Street Fighter 2010")
    "Super Xevious - Gump no Nazo": ["7BB5664F"],                                 # "Super Xevious꞉ Gampの謎" (Japan-only)
    "Thunder _ Lightning": ["D80B44BC"],                                          # "Thunder & Lightning" (NA)
    "Town _ Country Surf Designs - Thrilla's Surfari": ["7E57FBEC"],              # "T&C Surf Designs II꞉ Thrilla's Surfari"
    "Town _ Country Surf Designs - Wood _ Water Rage": ["D3BFF72E"],              # "T&C Surf Designs꞉ Wood and Water Rage"
    "Track _ Field": ["9C9F3571"],                                                # "Track & Field" (original, NA; NOT Track & Field II)
    "Ultima - Exodus": ["A4062017"],                                             # "Ultima III꞉ Exodus" (NA)
    "Ultima - Quest of the Avatar": ["A25A750F"],                                # "Ultima IV꞉ Quest of the Avatar" (NA)
    "Ultima - Warriors of Destiny": ["4823EEFE"],                                # "Ultima V꞉ Warriors of Destiny" (NA)
    "Ultimate Stuntman, The": ["892434DD"],                                      # Unlicensed NA "Ultimate Stuntman"
    "Wizards _ Warriors III - Kuros...Visions of Power": ["D2562072", "806DE21E"],  # NA + PAL
    "Xevious - The Avenger": ["DFD70E27", "D745D7CB"],                            # US "Xevious" NA + PAL
    "Zanac": ["E292AA10"],                                                        # "Zanac A.I." (NA)
}
