package com.doublegate.rustynes

import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Star
import androidx.compose.material.icons.outlined.StarBorder
import androidx.compose.material3.AssistChip
import androidx.compose.material3.Button
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import coil3.compose.AsyncImage

/**
 * The box-art game library grid (v1.8.8 "Atlas", Workstream C).
 *
 * Used in two places by the adaptive layout: as the **compact** idle screen (when no
 * ROM is loaded on a phone / folded cover screen) and as the **expanded-width** list
 * pane beside the player (tablet / unfolded foldable / desktop window). A box-art
 * tile grid with folder/favorite filters, name search, sort, and a long-press context
 * menu (favorite / set box art / move to folder / remove). Tap loads the ROM.
 *
 * Box art is user-supplied local content URIs only (Coil 3 loads `content://` without
 * a network fetcher). Games with no art get a generated placeholder tile.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun LibraryScreen(
    entries: List<GameEntry>,
    folders: List<String>,
    selectedFolder: String?,
    favoritesOnly: Boolean,
    query: String,
    sort: LibrarySort,
    onOpen: () -> Unit,
    onImportFolder: () -> Unit,
    onSelectFolder: (String?, Boolean) -> Unit,
    onQueryChange: (String) -> Unit,
    onSortChange: (LibrarySort) -> Unit,
    onPlay: (GameEntry) -> Unit,
    onToggleFavorite: (GameEntry) -> Unit,
    onSetBoxArt: (GameEntry) -> Unit,
    onMoveToFolder: (GameEntry) -> Unit,
    onRemove: (GameEntry) -> Unit,
    modifier: Modifier = Modifier,
    surfaceColor: Color = MaterialTheme.colorScheme.surface,
    onColor: Color = MaterialTheme.colorScheme.onSurface,
) {
    Column(
        modifier = modifier
            .background(surfaceColor)
            .padding(horizontal = 12.dp, vertical = 8.dp),
    ) {
        // Header: title + the Open / Import-folder actions.
        Row(
            modifier = Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Text(
                stringResource(R.string.label_library),
                color = onColor,
                fontSize = 20.sp,
                fontWeight = FontWeight.Bold,
                modifier = Modifier.weight(1f),
            )
            TextButton(onClick = onOpen) { Text(stringResource(R.string.action_open_rom)) }
            TextButton(onClick = onImportFolder) { Text(stringResource(R.string.library_import_folder)) }
        }

        // Search field.
        OutlinedTextField(
            value = query,
            onValueChange = onQueryChange,
            label = { Text(stringResource(R.string.library_search)) },
            singleLine = true,
            modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp),
        )

        // Collection filter: All / Favorites / + user folders.
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            FilterChip(
                selected = !favoritesOnly && selectedFolder == null,
                onClick = { onSelectFolder(null, false) },
                label = { Text(stringResource(R.string.library_all)) },
            )
            FilterChip(
                selected = favoritesOnly,
                onClick = { onSelectFolder(null, true) },
                label = { Text(stringResource(R.string.library_favorites)) },
            )
            folders.forEach { f ->
                FilterChip(
                    selected = !favoritesOnly && selectedFolder == f,
                    onClick = { onSelectFolder(f, false) },
                    label = { Text(f) },
                )
            }
        }

        // Sort selector.
        Row(
            modifier = Modifier.fillMaxWidth().padding(top = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            Text(stringResource(R.string.library_sort), color = onColor, fontSize = 12.sp)
            SortChip(stringResource(R.string.library_sort_recent), sort == LibrarySort.RECENT) {
                onSortChange(LibrarySort.RECENT)
            }
            SortChip(stringResource(R.string.library_sort_name), sort == LibrarySort.NAME) {
                onSortChange(LibrarySort.NAME)
            }
            SortChip(stringResource(R.string.library_sort_favorite), sort == LibrarySort.FAVORITE) {
                onSortChange(LibrarySort.FAVORITE)
            }
        }

        if (entries.isEmpty()) {
            Box(
                modifier = Modifier.fillMaxSize(),
                contentAlignment = Alignment.Center,
            ) {
                Text(
                    stringResource(R.string.library_empty),
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    textAlign = TextAlign.Center,
                )
            }
        } else {
            // The grid. Adaptive cell width keeps tiles a sensible size from a narrow
            // phone (2 columns) up to a wide tablet/desktop pane (many columns), and a
            // stable key (the ROM SHA) + uniform contentType keep recompositions cheap
            // (ties into the WS J baseline-profile / scroll-perf work).
            LazyVerticalGrid(
                columns = GridCells.Adaptive(minSize = 108.dp),
                modifier = Modifier.fillMaxSize().padding(top = 8.dp),
                horizontalArrangement = Arrangement.spacedBy(10.dp),
                verticalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                items(entries, key = { it.sha }, contentType = { "game" }) { entry ->
                    GameTile(
                        entry = entry,
                        onPlay = { onPlay(entry) },
                        onToggleFavorite = { onToggleFavorite(entry) },
                        onSetBoxArt = { onSetBoxArt(entry) },
                        onMoveToFolder = { onMoveToFolder(entry) },
                        onRemove = { onRemove(entry) },
                    )
                }
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun SortChip(label: String, selected: Boolean, onClick: () -> Unit) {
    FilterChip(selected = selected, onClick = onClick, label = { Text(label, fontSize = 12.sp) })
}

/**
 * A single box-art tile: the art (or a generated placeholder), the name, a favorite
 * star, and a long-press context menu (favorite / set box art / move to folder /
 * remove). Tap = play.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun GameTile(
    entry: GameEntry,
    onPlay: () -> Unit,
    onToggleFavorite: () -> Unit,
    onSetBoxArt: () -> Unit,
    onMoveToFolder: () -> Unit,
    onRemove: () -> Unit,
) {
    var menuOpen by remember { mutableStateOf(false) }
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .combinedClickable(
                onClick = onPlay,
                onLongClick = { menuOpen = true },
            ),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Box(
            modifier = Modifier
                .fillMaxWidth()
                // NES box art is roughly 5:7 portrait; keep tiles uniform regardless.
                .aspectRatio(0.72f)
                .clip(RoundedCornerShape(8.dp)),
            contentAlignment = Alignment.TopEnd,
        ) {
            if (entry.boxArtUri.isNotEmpty()) {
                AsyncImage(
                    model = entry.boxArtUri,
                    contentDescription = entry.name,
                    modifier = Modifier.fillMaxSize(),
                    contentScale = ContentScale.Crop,
                )
            } else {
                PlaceholderArt(entry.name)
            }
            // Favorite star overlay (filled = starred). Tap toggles directly.
            Box(
                modifier = Modifier
                    .padding(4.dp)
                    .clip(RoundedCornerShape(50))
                    .background(Color(0x99000000))
                    .combinedClickable(onClick = onToggleFavorite)
                    .padding(3.dp),
            ) {
                Icon(
                    imageVector = if (entry.favorite) Icons.Filled.Star else Icons.Outlined.StarBorder,
                    contentDescription = stringResource(R.string.library_favorite),
                    tint = if (entry.favorite) Color(0xFFFFD54F) else Color.White,
                    modifier = Modifier.size(18.dp),
                )
            }
            // The long-press context menu anchors to the tile.
            DropdownMenu(expanded = menuOpen, onDismissRequest = { menuOpen = false }) {
                DropdownMenuItem(
                    text = {
                        Text(
                            stringResource(
                                if (entry.favorite) R.string.library_unfavorite else R.string.library_favorite,
                            ),
                        )
                    },
                    onClick = { menuOpen = false; onToggleFavorite() },
                )
                DropdownMenuItem(
                    text = { Text(stringResource(R.string.library_set_box_art)) },
                    onClick = { menuOpen = false; onSetBoxArt() },
                )
                DropdownMenuItem(
                    text = { Text(stringResource(R.string.library_move_to_folder)) },
                    onClick = { menuOpen = false; onMoveToFolder() },
                )
                DropdownMenuItem(
                    text = { Text(stringResource(R.string.library_remove)) },
                    onClick = { menuOpen = false; onRemove() },
                )
            }
        }
        Text(
            entry.name,
            color = MaterialTheme.colorScheme.onSurface,
            fontSize = 12.sp,
            maxLines = 2,
            overflow = TextOverflow.Ellipsis,
            textAlign = TextAlign.Center,
            modifier = Modifier.fillMaxWidth().padding(top = 4.dp),
        )
    }
}

/**
 * A generated placeholder tile for a game with no box art: a colored panel (a stable
 * hue derived from the name) with the game's leading initials. No bundled assets.
 */
@Composable
private fun PlaceholderArt(name: String) {
    val hue = (name.hashCode().toUInt() % 360u).toFloat()
    val bg = Color.hsl(hue, 0.45f, 0.32f)
    val initials = name.split(Regex("[\\s_\\-.]+"))
        .filter { it.isNotEmpty() }
        .take(2)
        .joinToString("") { it.first().uppercaseChar().toString() }
        .ifEmpty { "?" }
    Box(
        modifier = Modifier.fillMaxSize().background(bg),
        contentAlignment = Alignment.Center,
    ) {
        Text(initials, color = Color.White, fontSize = 28.sp, fontWeight = FontWeight.Bold)
    }
}

/**
 * A small dialog to pick or type a folder/collection tag for a game (v1.8.8 WS C).
 * The existing [folders] are offered as quick chips; a free-text field creates a new
 * one. Clearing the text moves the game back to uncategorized.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun MoveToFolderDialog(
    current: String,
    folders: List<String>,
    onConfirm: (String) -> Unit,
    onDismiss: () -> Unit,
) {
    var text by remember { mutableStateOf(current) }
    androidx.compose.material3.AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.library_move_to_folder)) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                if (folders.isNotEmpty()) {
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        folders.forEach { f ->
                            AssistChip(onClick = { text = f }, label = { Text(f) })
                        }
                    }
                }
                OutlinedTextField(
                    value = text,
                    onValueChange = { text = it },
                    label = { Text(stringResource(R.string.library_folder_name)) },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                )
            }
        },
        confirmButton = {
            Button(onClick = { onConfirm(text.trim()) }) { Text(stringResource(R.string.library_move)) }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) { Text(stringResource(R.string.about_close)) }
        },
    )
}

/**
 * Box-art preview dialog (v1.8.8 WS C): shows the auto-matched cover (a `file://`
 * to the cached download, or a `content://` the user picked) before it is applied,
 * so a wrong match isn't silently saved. [state] drives the body: searching, a
 * found preview, or not-found.
 */
sealed interface BoxArtPreview {
    /** Network match in progress for [name]. */
    data class Searching(val name: String) : BoxArtPreview
    /** A candidate was found at [uri] (a cached `file://`). */
    data class Found(val uri: String, val name: String) : BoxArtPreview
    /** No match found for [name]. */
    data class NotFound(val name: String) : BoxArtPreview
}

@Composable
fun BoxArtPreviewDialog(
    state: BoxArtPreview,
    onApply: (String) -> Unit,
    onPickManually: () -> Unit,
    onDismiss: () -> Unit,
) {
    androidx.compose.material3.AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.library_box_art)) },
        text = {
            Column(
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.spacedBy(10.dp),
                modifier = Modifier.fillMaxWidth(),
            ) {
                when (state) {
                    is BoxArtPreview.Searching -> {
                        androidx.compose.material3.CircularProgressIndicator()
                        Text(stringResource(R.string.library_searching_art, state.name))
                    }
                    is BoxArtPreview.Found -> {
                        AsyncImage(
                            model = state.uri,
                            contentDescription = state.name,
                            modifier = Modifier
                                .fillMaxWidth(0.6f)
                                .aspectRatio(0.72f)
                                .clip(RoundedCornerShape(8.dp)),
                            contentScale = ContentScale.Fit,
                        )
                        Text(state.name, textAlign = TextAlign.Center)
                    }
                    is BoxArtPreview.NotFound ->
                        Text(stringResource(R.string.library_no_art_found, state.name))
                }
            }
        },
        confirmButton = {
            if (state is BoxArtPreview.Found) {
                Button(onClick = { onApply(state.uri) }) { Text(stringResource(R.string.library_apply)) }
            } else {
                TextButton(onClick = onPickManually) { Text(stringResource(R.string.library_pick_image)) }
            }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) { Text(stringResource(R.string.about_close)) }
        },
    )
}

/** A small import-progress banner (v1.8.8 WS C) — "Imported N of M…". */
@Composable
fun ImportProgressBanner(done: Int, total: Int, modifier: Modifier = Modifier) {
    Box(
        modifier = modifier.background(Color(0xC0102030)).padding(horizontal = 10.dp, vertical = 6.dp),
    ) {
        Text(
            stringResource(R.string.library_importing, done, total),
            color = Color.White,
            fontSize = 12.sp,
        )
    }
}
