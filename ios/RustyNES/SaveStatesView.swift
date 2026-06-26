//
//  SaveStatesView.swift
//
//  The save-state manager: the four per-ROM slots laid out as cards, each showing
//  a thumbnail (the framebuffer at save time), the timestamp, and the captured
//  frame index, with Save / Load / Delete per slot. Presented as a sheet from the
//  in-game pill menu. Mirrors the Android save-state manager.
//
//  Slots are keyed by the ROM's SHA-256 under the sandbox (see SaveStateManager);
//  the `.rns` blob format is shared with desktop / Android.
//
//  v1.9.7: each slot shows a small iCloud sync indicator (synced / uploading /
//  local-only) when CloudKit save-state sync is enabled, observing CloudSaveStateSync
//  directly so it updates live as uploads / reconciliation complete.
//

import SwiftUI
import UIKit

struct SaveStatesView: View {
    @EnvironmentObject private var model: AppModel

    // Observe the cloud-sync model directly (AppModel doesn't republish nested
    // ObservableObject changes), so the per-slot indicators refresh live.
    var body: some View {
        SaveStatesContent(cloud: model.cloudSaveStates)
    }
}

private struct SaveStatesContent: View {
    @EnvironmentObject private var model: AppModel
    @Environment(\.dismiss) private var dismiss
    @ObservedObject var cloud: CloudSaveStateSync

    // A local snapshot of the slot state, reloaded after every mutation (the slots
    // are file-backed, not @Published, so the view refreshes them explicitly).
    @State private var slots: [SaveSlot] = []
    // The slot pending deletion (drives the confirmation dialog).
    @State private var slotToDelete: Int?

    private let columns = [GridItem(.adaptive(minimum: 150), spacing: 16)]

    var body: some View {
        NavigationStack {
            ScrollView {
                LazyVGrid(columns: columns, spacing: 16) {
                    ForEach(slots) { slot in
                        SlotCard(
                            slot: slot,
                            syncState: cloud.state(for: slot.index),
                            onSave: { save(slot.index) },
                            onLoad: { load(slot.index) },
                            onDelete: { slotToDelete = slot.index }
                        )
                    }
                }
                .padding()
            }
            .navigationTitle("Save States")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") { dismiss() }
                }
            }
            .onAppear(perform: reload)
            // A cloud reconcile can overwrite a local slot (newer-remote pull); refresh
            // the file-backed snapshot so its thumbnail / timestamp update too.
            .onChange(of: cloud.states) { _ in reload() }
            .confirmationDialog(
                "Delete this save state?",
                isPresented: Binding(
                    get: { slotToDelete != nil },
                    set: { if !$0 { slotToDelete = nil } }
                ),
                titleVisibility: .visible
            ) {
                Button("Delete", role: .destructive) {
                    if let i = slotToDelete { delete(i) }
                }
                Button("Cancel", role: .cancel) { slotToDelete = nil }
            }
        }
    }

    private func reload() { slots = model.slots() }

    private func save(_ index: Int) {
        model.saveSlot(index)
        reload()
    }

    private func load(_ index: Int) {
        model.loadSlot(index)
        dismiss()
    }

    private func delete(_ index: Int) {
        model.deleteSlot(index)
        slotToDelete = nil
        reload()
    }
}

/// One slot card: thumbnail (or an empty placeholder), metadata, and actions.
private struct SlotCard: View {
    let slot: SaveSlot
    let syncState: CloudSaveStateSync.SlotSyncState
    let onSave: () -> Void
    let onLoad: () -> Void
    let onDelete: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            ZStack(alignment: .topTrailing) {
                ZStack {
                    RoundedRectangle(cornerRadius: 10)
                        .fill(Color.secondary.opacity(0.18))
                        .aspectRatio(256.0 / 240.0, contentMode: .fit)
                    if let image = slot.thumbnail {
                        Image(uiImage: image)
                            .resizable()
                            .interpolation(.none)
                            .aspectRatio(256.0 / 240.0, contentMode: .fit)
                            .clipShape(RoundedRectangle(cornerRadius: 10))
                    } else {
                        Image(systemName: slot.isEmpty ? "square.dashed" : "photo")
                            .font(.system(size: 30))
                            .foregroundStyle(.secondary)
                            .accessibilityHidden(true) // decorative placeholder
                    }
                }
                // iCloud sync indicator (only for non-empty slots while sync is on).
                if !slot.isEmpty, let badge = syncBadge {
                    Image(systemName: badge.symbol)
                        .font(.caption)
                        .foregroundStyle(badge.color)
                        .padding(5)
                        .background(.ultraThinMaterial, in: Circle())
                        .padding(6)
                        .accessibilityLabel(badge.label)
                }
            }

            Text("Slot \(slot.index + 1)")
                .font(.headline)
            Text(subtitle)
                .font(.caption)
                .foregroundStyle(.secondary)
                .lineLimit(2)

            HStack(spacing: 8) {
                Button("Save", action: onSave)
                    .buttonStyle(.bordered)
                    .accessibilityLabel("Save to slot \(slot.index + 1)")
                Button("Load", action: onLoad)
                    .buttonStyle(.borderedProminent)
                    .disabled(slot.isEmpty)
                    .accessibilityLabel("Load slot \(slot.index + 1)")
                Spacer(minLength: 0)
                Button(role: .destructive, action: onDelete) {
                    Image(systemName: "trash")
                }
                .buttonStyle(.borderless)
                .disabled(slot.isEmpty)
                .accessibilityLabel("Delete slot \(slot.index + 1)")
            }
            .font(.footnote)
        }
        .padding(12)
        .background(Color(.secondarySystemGroupedBackground), in: RoundedRectangle(cornerRadius: 14))
    }

    private var subtitle: String {
        guard let savedAt = slot.savedAt else { return "Empty" }
        let when = savedAt.formatted(date: .abbreviated, time: .shortened)
        if let frame = slot.frame, frame > 0 {
            return "\(when)\nFrame \(frame)"
        }
        return when
    }

    /// The iCloud indicator (symbol + colour + label) for this slot, or nil to hide it
    /// (sync disabled / iCloud unavailable).
    private var syncBadge: (symbol: String, color: Color, label: String)? {
        switch syncState {
        case .unavailable: return nil
        case .uploading: return ("arrow.up.circle", .secondary, "Uploading to iCloud")
        case .synced: return ("icloud.fill", .accentColor, "Synced to iCloud")
        case .localOnly: return ("icloud.slash", .secondary, "On this device only")
        }
    }
}
