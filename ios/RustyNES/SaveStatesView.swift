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

import SwiftUI
import UIKit

struct SaveStatesView: View {
    @EnvironmentObject private var model: AppModel
    @Environment(\.dismiss) private var dismiss

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
    let onSave: () -> Void
    let onLoad: () -> Void
    let onDelete: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
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
                Button("Load", action: onLoad)
                    .buttonStyle(.borderedProminent)
                    .disabled(slot.isEmpty)
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
}
