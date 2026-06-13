# FLOOR baseline (mc-r1-full-cpu, HEAD 88df82a)
AccuracyCoin RAM = 94.96% / 7 fails:
  1. Interrupt flag latency [error 10]
  2. DMC DMA + OAM DMA [error 2]   <- M
  3. Explicit DMA Abort [error 2]  <- M
  4. Delta Modulation Channel [error 18]
  5. APU Register Activation [error 6]
  6. $2007 Stress Test [error 2]   (Program P)
  7. Implied Dummy Reads [error 5]
SH* = 6/6 PASS
C1: cpu_interrupts_v2 #1 pass, #4 pass, #5 pass; #2/#3 strict ignored but PASS
    (the _currently_fails probes "FAIL" with "unexpectedly PASSES" = C1-GREEN signature)

# scan_dma_abort FLOOR arrays:
DMCOAM $50-$5F floor = 03 04 03 04 03 04 03 04 03 04 03 04 03 04 03 04
DMCOAM $50-$5F KEY   = 04 03 04 03 04 03 02 01 02 01 02 01 02 01 02 01
EXPLICIT $50-$5F floor= 04 04 04 04 04 04 03 03 00 01 01 00 00 00 00 00
EXPLICIT $50-$5F KEY  = 04 04 04 04 04 04 03 04 01 01 00 00 00 00 00 00
IMPLICIT $500 = KEY1, $520=KEY2, $540=KEY3 (PASS)
CheckDMATiming Y (DMC+OAM-measured) = 3
