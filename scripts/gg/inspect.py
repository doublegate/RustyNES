#!/usr/bin/env python3
"""Spot-print the NA release of each classic: prg/chr CRCs, the header-excluded
<rom crc32>, and the combine(prg,chr) identity result, reusing verify's parse."""
from crc_combine import crc32_combine
from verify import entries  # reuse parsed entries

for key in ["Super Mario Bros.", "The Legend of Zelda", "Metroid", "Castlevania", "Contra"]:
    for e in entries:
        if key in e["name"] and "North America" in e["name"] and "Bros. 2" not in e["name"] and "Bros. 3" not in e["name"] and "Zelda 2" not in e["name"] and "II" not in e["name"]:
            prg, chrc, chsz, rom = e["prg_crc"], e["chr_crc"], e["chr_sz"], e["rom_crc"]
            comb = crc32_combine(prg, chrc, chsz) if chsz and chrc is not None else prg
            print(f"{e['name']:<52} prg={e['prg_crc']:08X} chr={(e['chr_crc'] or 0):08X} chrsz={chsz:<6} "
                  f"rom={rom:08X} combine(prg,chr)={comb:08X}  combine==rom:{comb==rom}")
