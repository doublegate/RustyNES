## Iter 1: put_cycle end-flip + byte-timer on !put_cycle (counter-collapse v1)
CheckDMATiming Y = 4 (floor 3) -- GATE MOVED
DMCOAM $50-$5F = 04 03 04 03 04 03 02 03 02 01 02 01 02 01 02 01
        KEY    = 04 03 04 03 04 03 02 01 02 01 02 01 02 01 02 01
  -> matches idx0-6 + idx8-15; ONLY idx[7] off (mine 03 vs KEY 01). ~NEAR CLOSE
EXPLICIT $50-$5F = 05 05 05 05 05 05 05 01 02 02 01 01 01 01 01 01
         KEY     = 04 04 04 04 04 04 03 04 01 01 00 00 00 00 00 00
  -> OVER-SHIFTED (05 spans) -- regressed
IMPLICIT $540 = 01...01 04 04 03 03 05 05 05  (KEY3 00...00 04x6) -- regressed
