package com.qrtx.transceiver

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.CancellationException
import org.json.JSONObject

@Composable
fun TransferList(transfers: List<Transfer>, onOpen: (String) -> Unit) {
    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        item {
            Text("Your files", style = MaterialTheme.typography.headlineMedium)
            Text("Queues and partial transfers are saved on this device.", style = MaterialTheme.typography.bodyMedium)
        }
        if (transfers.isEmpty()) item {
            SectionCard("No transfers yet", "Add files on the Transfer screen, or start the camera to receive.") {}
        }
        items(transfers, key = { it.id }) { transfer -> TransferCard(transfer) { onOpen(transfer.id) } }
    }
}

@Composable
fun TransferCard(transfer: Transfer, onOpen: () -> Unit) {
    Card(onClick = onOpen, modifier = Modifier.fillMaxWidth()) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(if (transfer.direction == "send") "OUTGOING" else "INCOMING", style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.primary)
            Text(transfer.title, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
            Text("${transfer.files.count { !it.directory }} files · ${formatBytes(transfer.totalBytes)}", style = MaterialTheme.typography.bodySmall)
            if (transfer.direction == "receive") LinearProgressIndicator(progress = { transfer.progress }, modifier = Modifier.fillMaxWidth())
            Text(transfer.statusLabel, style = MaterialTheme.typography.labelMedium)
            if (transfer.direction == "send") Text("${transfer.cycles} loops · ${transfer.frames} frames scheduled", style = MaterialTheme.typography.bodySmall)
            transfer.error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
        }
    }
}

@Composable
fun TransferDetails(transfer: Transfer, model: TransferViewModel, onBack: () -> Unit, onExportFolder: () -> Unit, onExport: (TransferFile) -> Unit) {
    var inspectFile by rememberSaveable(transfer.id) { mutableStateOf<Int?>(null) }
    var confirmRemove by remember { mutableStateOf(false) }
    if (confirmRemove) AlertDialog(
        onDismissRequest = { confirmRemove = false },
        title = { Text("Remove this transfer?") },
        text = { Text("This clears the queue entry and private file copies. Files already exported remain saved.") },
        confirmButton = { TextButton(onClick = { confirmRemove = false; model.action(transfer.id, "remove"); onBack() }) { Text("Remove") } },
        dismissButton = { TextButton(onClick = { confirmRemove = false }) { Text("Keep") } }
    )
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        TextButton(onClick = onBack) { Text("‹ All files") }
        Text(transfer.title, style = MaterialTheme.typography.headlineSmall)
        Text(transfer.statusLabel, color = MaterialTheme.colorScheme.primary)
        SelectionContainer { Text(transfer.id, fontFamily = FontFamily.Monospace, style = MaterialTheme.typography.labelSmall) }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            if (transfer.status == "active") {
                OutlinedButton(onClick = { model.action(transfer.id, "pause") }) { Text("Pause") }
                TextButton(onClick = { model.action(transfer.id, "cancel") }) { Text("Cancel") }
            } else {
                if (transfer.status == "paused" || transfer.status == "cancelled" || transfer.status == "failed" || transfer.status == "displayed") {
                    OutlinedButton(onClick = { model.action(transfer.id, "resume") }) { Text("Resume") }
                }
                TextButton(onClick = { confirmRemove = true }) { Text("Remove") }
            }
            if (transfer.direction == "send" && transfer.status != "active" && !transfer.passwordBundle) {
                TextButton(onClick = { model.action(transfer.id, "restart") }) { Text("Restart") }
            }
        }
        if (transfer.direction == "receive" && transfer.status == "active") {
            Text("${formatBytes(transfer.bytesPerSecond.toLong())}/s useful data · ${(transfer.progress * 100).toInt()}% received")
        }
        if (transfer.direction == "receive" && transfer.status == "complete") {
            Button(onClick = onExportFolder, modifier = Modifier.fillMaxWidth()) { Text("Save all to folder") }
        }
        transfer.files.filter { !it.directory }.forEach { file ->
            SectionCard(file.name, "${formatBytes(file.size)} · ${if (file.verified) "SHA-256 verified" else "Verification pending"}") {
                SelectionContainer {
                    Text(file.sha256, fontFamily = FontFamily.Monospace, style = MaterialTheme.typography.labelSmall)
                }
                if (transfer.direction == "receive") Text("${file.receivedChunks} / ${file.totalChunks} chunks received", style = MaterialTheme.typography.bodySmall)
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    if (transfer.direction == "receive" && file.verified) Button(onClick = { onExport(file) }) { Text("Save file") }
                    TextButton(onClick = { inspectFile = if (inspectFile == file.id) null else file.id }) { Text("Inspect bytes") }
                }
                if (transfer.direction == "receive" && !file.verified) {
                    TextButton(onClick = { model.requestMissing(transfer.id, file.id) }) { Text("Request missing chunks") }
                }
                if (inspectFile == file.id) PayloadInspector(transfer.id, file, model)
            }
        }
        if (transfer.files.all { it.directory }) Text("This transfer contains only empty folders.")
    }
}

@Composable
fun ReceivePasswordInput(model: TransferViewModel, id: String? = null, enabled: Boolean = true) {
    // Deliberately remember, not rememberSaveable: passwords never enter saved state.
    var password by remember(id) { mutableStateOf("") }
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        OutlinedTextField(value = password, onValueChange = { password = it },
            label = { Text(if (id == null) "Receive password (optional)" else "Transfer password") },
            visualTransformation = PasswordVisualTransformation(), keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password),
            singleLine = true, enabled = enabled, modifier = Modifier.fillMaxWidth())
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Button(onClick = { model.receivePassword(id, password); password = "" }, enabled = enabled && password.isNotEmpty()) {
                Text(if (id == null) "Use password" else "Unlock transfer")
            }
            if (id == null) TextButton(onClick = { password = ""; model.receivePassword(null, null) }, enabled = enabled) { Text("Clear") }
        }
        Text(if (id == null) "Enter before scanning or when a bundle arrives. Passwords are never saved." else "Scanning continues. Retry without rescanning.", style = MaterialTheme.typography.bodySmall)
    }
}

@Composable
private fun PayloadInspector(id: String, file: TransferFile, model: TransferViewModel) {
    var offset by rememberSaveable(id, file.id) { mutableLongStateOf(0L) }
    var page by remember { mutableStateOf<JSONObject?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(id, file.id, offset, file.receivedBytes) {
        try { page = model.payload(id, file.id, offset); error = null }
        catch (e: Exception) {
            if (e is CancellationException) throw e
            error = e.message ?: "Payload unavailable"
        }
    }
    error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
    val data = page?.optJSONArray("bytes")
    if (data != null) {
        val text = buildString {
            for (row in 0 until data.length() step 8) {
                append("%08x  ".format(offset + row))
                for (index in row until minOf(row + 8, data.length())) {
                    append(if (data.isNull(index)) "·· " else "%02x ".format(data.getInt(index)))
                }
                appendLine()
                append("          ")
                for (index in row until minOf(row + 8, data.length())) {
                    val value = if (data.isNull(index)) -1 else data.getInt(index)
                    append(if (value in 32..126) value.toChar() else '·')
                }
                appendLine()
            }
        }
        SelectionContainer { Text(text.ifEmpty { "Empty file" }, fontFamily = FontFamily.Monospace, style = MaterialTheme.typography.labelSmall) }
        Text("Hex bytes and ASCII · dots mark missing or nonprinting data.", style = MaterialTheme.typography.labelSmall)
    }
    Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        TextButton(onClick = { offset = (offset - 256).coerceAtLeast(0) }, enabled = offset > 0) { Text("Previous") }
        TextButton(onClick = { offset += 256 }, enabled = offset + 256 < file.size) { Text("Next") }
    }
}

@Composable
fun SettingsScreen(state: AppState, onSave: (TransferSettings) -> Unit) {
    var frameMs by rememberSaveable(state.settings.frameMs) { mutableStateOf(state.settings.frameMs.toString()) }
    var chunkSize by rememberSaveable(state.settings.chunkSize) { mutableStateOf(state.settings.chunkSize.toString()) }
    var correction by rememberSaveable(state.settings.correction) { mutableStateOf(state.settings.correction) }
    var peers by rememberSaveable(state.settings.trustedPeers.joinToString()) { mutableStateOf(state.settings.trustedPeers.joinToString("\n")) }
    val normalizedPeers = peers.split(Regex("[\\s,;]+")).filter { it.isNotBlank() }.map { it.lowercase() }.distinct()
    val valid = frameMs.toIntOrNull() in 50..60000 && chunkSize.toIntOrNull() in 64..1168 &&
        normalizedPeers.all { it.matches(Regex("[0-9a-f]{64}")) }
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
        Text("Transfer settings", style = MaterialTheme.typography.headlineMedium)
        SectionCard("QR sequence", "Slower frames and smaller chunks are easier for cameras to read.") {
            OutlinedTextField(
                value = frameMs, onValueChange = { frameMs = it }, label = { Text("Frame duration (ms)") },
                supportingText = { Text("50–60,000 ms. Applies to queued transfers.") },
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number), singleLine = true, modifier = Modifier.fillMaxWidth()
            )
            OutlinedTextField(
                value = chunkSize, onValueChange = { chunkSize = it }, label = { Text("Chunk size (bytes)") },
                supportingText = { Text("64–1,168 bytes. Used for new transfers.") },
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number), singleLine = true, modifier = Modifier.fillMaxWidth()
            )
            Text("Error correction", style = MaterialTheme.typography.labelLarge)
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                listOf("L", "M", "Q", "H").forEach { value ->
                    FilterChip(selected = value == correction, onClick = { correction = value }, label = { Text(value) })
                }
            }
        }
        SectionCard("Trusted receipts", "Compare fingerprints directly on both devices before adding files to the queue.") {
            Text("This device", style = MaterialTheme.typography.labelLarge)
            SelectionContainer { Text(state.fingerprint, fontFamily = FontFamily.Monospace, style = MaterialTheme.typography.bodySmall) }
            OutlinedTextField(
                value = peers, onValueChange = { peers = it }, label = { Text("Trusted peer fingerprints") },
                supportingText = { Text("One 64-character fingerprint per line. Long-press your fingerprint above to copy it.") },
                minLines = 3, modifier = Modifier.fillMaxWidth()
            )
            Text("Only a receipt signed by a trusted peer can confirm delivery. Trust changes apply to newly queued transfers.", style = MaterialTheme.typography.bodySmall)
        }
        Button(
            onClick = { onSave(state.settings.copy(frameMs = frameMs.toInt(), chunkSize = chunkSize.toInt(), correction = correction, trustedPeers = normalizedPeers)) },
            enabled = valid && !state.busy, modifier = Modifier.fillMaxWidth()
        ) { Text("Save settings") }
        Text("Android 8+ · QRTX/1\nReceived files are verified in private storage before you save them elsewhere. The incoming queue is limited to 1 GiB; remove saved transfers to free capacity. Imports and received data are removed when the app is uninstalled.", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}
