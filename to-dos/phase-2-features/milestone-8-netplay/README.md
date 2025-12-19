# Milestone 8: Netplay (GGPO)

**Phase:** 2 (Advanced Features)
**Duration:** Months 7-9
**Status:** Planned
**Target:** September 2026

---

## Overview

Implement rollback netcode using backroll-rs (Rust GGPO port). This milestone adds multiplayer capabilities with minimal input lag and robust synchronization.

## Goals

- [ ] backroll-rs integration (Rust GGPO port)
- [ ] Save state serialization
- [ ] Input prediction/rollback
- [ ] Lobby system
- [ ] Spectator mode
- [ ] NAT traversal (STUN/TURN)

## Acceptance Criteria

- [ ] 1-2 frame input lag over LAN
- [ ] <5 frame rollback on 100ms ping
- [ ] No desyncs in 30-minute sessions
- [ ] Works behind typical NAT setups

## Dependencies

- Save states functional
- Deterministic emulation verified
- GUI framework ready for netplay UI

---

## Future Planning

**Detailed tasks to be created when milestone begins**
