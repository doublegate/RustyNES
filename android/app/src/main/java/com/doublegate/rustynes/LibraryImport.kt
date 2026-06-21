package com.doublegate.rustynes

import android.content.Context
import android.net.Uri
import android.provider.DocumentsContract

/**
 * Batch SAF-tree ROM import (v1.8.8 "Atlas", Workstream C).
 *
 * Given a tree URI from `ACTION_OPEN_DOCUMENT_TREE` (with a persistable grant already
 * taken by the caller), enumerate the directory (one level, plus immediate
 * subfolders) for NES ROM files (`.nes` / `.fds` / `.unf` / `.unif` / `.zip`),
 * register each in the [GameLibrary] keyed by its real ROM SHA-256, and auto-link a
 * sibling box-art image (`<romname>.png` / `.jpg` / `.jpeg` / `.webp`) when present.
 *
 * Runs entirely off the main thread (the caller wraps it in `Dispatchers.IO`). Dedups
 * by SHA — re-importing the same folder updates the existing entries rather than
 * duplicating them. Returns the number of ROMs added/updated.
 */
object LibraryImport {
    private val ROM_EXTS = setOf("nes", "fds", "unf", "unif", "zip")
    private val IMG_EXTS = setOf("png", "jpg", "jpeg", "webp")

    private data class Child(val uri: Uri, val name: String, val isDir: Boolean)

    /**
     * Import [treeUri]. [onProgress] is invoked as `(done, total)` while scanning so
     * the caller can show a banner. Returns the count of ROMs registered.
     */
    fun importTree(
        ctx: Context,
        treeUri: Uri,
        onProgress: (done: Int, total: Int) -> Unit,
    ): Int {
        val resolver = ctx.contentResolver
        // Gather ROM + image children across the root and its immediate subfolders.
        val roms = mutableListOf<Child>()
        // Map a lowercase base name (no ext) -> the image child, for sibling matching.
        val imagesByBase = mutableMapOf<String, Uri>()

        fun scan(parentDocId: String) {
            val childrenUri = DocumentsContract.buildChildDocumentsUriUsingTree(treeUri, parentDocId)
            val subdirs = mutableListOf<String>()
            runCatching {
                resolver.query(
                    childrenUri,
                    arrayOf(
                        DocumentsContract.Document.COLUMN_DOCUMENT_ID,
                        DocumentsContract.Document.COLUMN_DISPLAY_NAME,
                        DocumentsContract.Document.COLUMN_MIME_TYPE,
                    ),
                    null, null, null,
                )?.use { c ->
                    val idIdx = c.getColumnIndexOrThrow(DocumentsContract.Document.COLUMN_DOCUMENT_ID)
                    val nameIdx = c.getColumnIndexOrThrow(DocumentsContract.Document.COLUMN_DISPLAY_NAME)
                    val mimeIdx = c.getColumnIndexOrThrow(DocumentsContract.Document.COLUMN_MIME_TYPE)
                    while (c.moveToNext()) {
                        val docId = c.getString(idIdx)
                        val name = c.getString(nameIdx) ?: continue
                        val mime = c.getString(mimeIdx)
                        val isDir = mime == DocumentsContract.Document.MIME_TYPE_DIR
                        val uri = DocumentsContract.buildDocumentUriUsingTree(treeUri, docId)
                        if (isDir) {
                            subdirs.add(docId)
                        } else {
                            val ext = name.substringAfterLast('.', "").lowercase()
                            val base = name.substringBeforeLast('.').lowercase()
                            when {
                                ext in ROM_EXTS -> roms.add(Child(uri, name, false))
                                ext in IMG_EXTS -> imagesByBase[base] = uri
                            }
                        }
                    }
                }
            }
            // One level of subfolders (a typical "ROMs/<system>/" layout) — bounded to
            // avoid pathological deep trees; deeper nesting is a TODO if requested.
            subdirs.forEach { scan(it) }
        }

        val rootDocId = DocumentsContract.getTreeDocumentId(treeUri)
        scan(rootDocId)

        val total = roms.size
        var added = 0
        roms.forEachIndexed { i, rom ->
            runCatching {
                val bytes = resolver.openInputStream(rom.uri)?.use { it.readBytes() }
                if (bytes != null && bytes.isNotEmpty()) {
                    val sha = sha256Hex(bytes)
                    val display = rom.name
                    // Sibling box-art match by base name (case-insensitive).
                    val base = rom.name.substringBeforeLast('.').lowercase()
                    val art = imagesByBase[base]
                    if (art != null) {
                        runCatching {
                            resolver.takePersistableUriPermission(
                                art,
                                android.content.Intent.FLAG_GRANT_READ_URI_PERMISSION,
                            )
                        }
                    }
                    GameLibrary.upsert(
                        ctx,
                        GameEntry(
                            sha = sha,
                            name = display,
                            uri = rom.uri.toString(),
                            boxArtUri = art?.toString() ?: "",
                        ),
                    )
                    added++
                }
            }
            onProgress(i + 1, total)
        }
        return added
    }
}
