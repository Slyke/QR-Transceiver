package com.qrtx.transceiver

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import androidx.core.net.toUri
import android.provider.Settings
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.FilterQuality
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import kotlin.math.floor

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun TransceiverApp(state: AppState, model: TransferViewModel) {
    var page by rememberSaveable { mutableStateOf("Transfer") }
    var fullscreen by rememberSaveable { mutableStateOf(false) }
    var selectedId by rememberSaveable { mutableStateOf<String?>(null) }
    var exportJob by rememberSaveable { mutableStateOf("") }
    var exportFile by rememberSaveable { mutableIntStateOf(0) }
    var permissionDenied by rememberSaveable { mutableStateOf(false) }
    val context = LocalContext.current
    val snackbars = remember { SnackbarHostState() }
    val files = rememberLauncherForActivityResult(ActivityResultContracts.OpenMultipleDocuments(), model::importFiles)
    val folder = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocumentTree()) { it?.let(model::importFolder) }
    val saveFolder = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocumentTree()) {
        if (it != null) model.exportFolder(exportJob, it)
    }
    val save = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/octet-stream")) {
        if (it != null) model.export(exportJob, exportFile, it)
    }
    val permission = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
        permissionDenied = !granted
        model.scan(granted)
    }
    val startCamera = {
        if (ContextCompat.checkSelfPermission(context, Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED) model.scan(true)
        else permission.launch(Manifest.permission.CAMERA)
    }
    LaunchedEffect(state.message) {
        state.message?.let { snackbars.showSnackbar(it); model.clearMessage() }
    }
    BackHandler(fullscreen || selectedId != null) {
        if (fullscreen) fullscreen = false else selectedId = null
    }
    if (state.error != null) AlertDialog(
        onDismissRequest = model::clearError,
        title = { Text("Action couldn’t finish") }, text = { Text(state.error) },
        confirmButton = { TextButton(onClick = model::clearError) { Text("OK") } }
    )
    if (permissionDenied) AlertDialog(
        onDismissRequest = { permissionDenied = false },
        title = { Text("Camera access needed") },
        text = { Text("Allow camera access to receive files and scan receipts. Sending files works without it.") },
        confirmButton = { TextButton(onClick = {
            permissionDenied = false
            context.startActivity(Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, "package:${context.packageName}".toUri()))
        }) { Text("App settings") } },
        dismissButton = { TextButton(onClick = { permissionDenied = false }) { Text("Close") } }
    )
    Scaffold(
        containerColor = if (fullscreen) Color.Black else MaterialTheme.colorScheme.background,
        topBar = { if (!fullscreen) TopAppBar(
            title = { Column { Text("QR Transceiver", fontWeight = FontWeight.Bold); Text("OFFLINE · OPTICAL TRANSFER", style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.primary) } }
        ) },
        bottomBar = { if (!fullscreen && selectedId == null) NavigationBar {
            listOf("Transfer" to "⇄", "Files" to "▤", "Settings" to "⚙").forEach { (title, symbol) ->
                NavigationBarItem(selected = page == title, onClick = {
                    if (page != title) { model.display(false); model.scan(false); page = title }
                }, icon = { Text(symbol) }, label = { Text(title) })
            }
        } },
        snackbarHost = { SnackbarHost(snackbars) }
    ) { padding ->
        if (!state.ready) {
            Box(Modifier.fillMaxSize().padding(padding), contentAlignment = Alignment.Center) {
                Column(horizontalAlignment = Alignment.CenterHorizontally) {
                    CircularProgressIndicator()
                    Text("Opening transfer engine…", Modifier.padding(24.dp))
                }
            }
        } else if (fullscreen) {
            Column(Modifier.fillMaxSize().padding(padding).background(Color.Black).padding(12.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                Box(Modifier.weight(1f).fillMaxWidth(), contentAlignment = Alignment.Center) { OpticalFrame(state.frame, Modifier.fillMaxSize()) }
                if (state.scanning) CameraPreview(
                    front = state.frontCamera, modifier = Modifier.width(128.dp).height(96.dp),
                    onFrame = model::analyze, onError = model::cameraError
                )
                Text(state.frame?.label ?: "Waiting for a frame", color = Color.White, style = MaterialTheme.typography.labelSmall)
                Row {
                    TextButton(onClick = { model.display(!state.displaying) }, colors = ButtonDefaults.textButtonColors(contentColor = Color(0xFF98E6CA))) { Text(if (state.displaying) "Pause display" else "Start display") }
                    TextButton(onClick = { fullscreen = false }, colors = ButtonDefaults.textButtonColors(contentColor = Color(0xFF98E6CA))) { Text("Exit full screen") }
                }
            }
        } else {
            val selected = state.transfers.find { it.id == selectedId }
            Column(Modifier.fillMaxSize().padding(padding)) {
                if (state.busy) {
                    LinearProgressIndicator(Modifier.fillMaxWidth())
                    Text(state.busyLabel, Modifier.padding(horizontal = 16.dp, vertical = 4.dp), style = MaterialTheme.typography.labelMedium)
                }
                state.transfers.filter { it.passwordNeeded && it.status in listOf("active", "locked") }.forEach { transfer ->
                    SectionCard("Encrypted transfer · ${transfer.id.take(8)}", "Enter its password while scanning continues.") {
                        ReceivePasswordInput(model, transfer.id, !state.busy)
                        transfer.error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
                    }
                }
                when {
                    selected != null -> TransferDetails(selected, model, onBack = { selectedId = null }, onExportFolder = { exportJob = selected.id; saveFolder.launch(null) }, onExport = { file ->
                        exportJob = selected.id; exportFile = file.id; save.launch(file.name.substringAfterLast('/'))
                    })
                    page == "Files" -> TransferList(state.transfers, onOpen = { selectedId = it })
                    page == "Settings" -> SettingsScreen(state, model::configure)
                    else -> Column(
                        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp),
                        verticalArrangement = Arrangement.spacedBy(16.dp)
                    ) {
                        Text("Transfer files", style = MaterialTheme.typography.headlineSmall, fontWeight = FontWeight.SemiBold)
                        Text("Point the receiving camera at the sending screen.", style = MaterialTheme.typography.bodyMedium)
                        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            listOf("send", "receive", "both").forEach { mode ->
                                FilterChip(
                                    selected = state.settings.mode == mode, onClick = { model.mode(mode) }, enabled = !state.busy,
                                    label = { Text(mode.replaceFirstChar { it.uppercase() }) }, modifier = Modifier.weight(1f)
                                )
                            }
                        }
                        if (state.settings.mode != "receive") {
                            SectionCard("Send through your screen", "${state.transfers.count { it.direction == "send" && it.status == "active" }} in the queue") {
                                Row(verticalAlignment = Alignment.CenterVertically) {
                                    Checkbox(checked = state.encryptSend, onCheckedChange = model::encryptSend, enabled = !state.busy)
                                    Text("Encrypt new transfer with a password")
                                }
                                if (state.encryptSend) {
                                    OutlinedTextField(value = state.sendPassword, onValueChange = model::sendPassword,
                                        label = { Text("Send password") }, visualTransformation = PasswordVisualTransformation(),
                                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password), singleLine = true,
                                        enabled = !state.busy, modifier = Modifier.fillMaxWidth())
                                    Text("Enter a password, then add files. Empty folders are omitted.", style = MaterialTheme.typography.bodySmall)
                                }
                                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                                    Button(onClick = { files.launch(arrayOf("*/*")) }, enabled = !state.busy && (!state.encryptSend || state.sendPassword.isNotEmpty())) { Text("Add files") }
                                    OutlinedButton(onClick = { folder.launch(null) }, enabled = !state.busy && (!state.encryptSend || state.sendPassword.isNotEmpty())) { Text("Add folder") }
                                }
                                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                                    Button(onClick = { model.display(!state.displaying) }) { Text(if (state.displaying) "Pause display" else "Start display") }
                                    TextButton(onClick = { fullscreen = true }) { Text("Full screen") }
                                }
                                OpticalFrame(state.frame, Modifier.fillMaxWidth().then(
                                    if (state.frame == null) Modifier.height(140.dp) else Modifier.aspectRatio(1f)
                                ))
                                state.frame?.let { Text(it.label, style = MaterialTheme.typography.labelSmall) }
                                Text("A completed loop does not confirm delivery.", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                            }
                        }
                        if (state.settings.mode != "send") {
                            SectionCard("Receive with your camera", if (state.frontCamera) "Front camera · useful for two-way transfers" else "Rear camera · tap preview to focus") {
                                ReceivePasswordInput(model, enabled = !state.busy)
                                if (state.scanning) CameraPreview(
                                    front = state.frontCamera, modifier = Modifier.fillMaxWidth().aspectRatio(4f / 3f),
                                    onFrame = model::analyze, onError = model::cameraError
                                )
                                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                                    Button(onClick = { if (state.scanning) model.scan(false) else startCamera() }) { Text(if (state.scanning) "Stop camera" else "Start camera") }
                                    TextButton(onClick = model::switchCamera) { Text(if (state.frontCamera) "Use rear" else "Use front") }
                                }
                                Text("${state.accepted} accepted · ${state.rejected} ignored frames", style = MaterialTheme.typography.labelSmall)
                                state.lastScanIssue?.let { Text("Last ignored frame: $it", style = MaterialTheme.typography.bodySmall) }
                                if (state.settings.mode == "both") Text("Start the display to return receipts and request missing chunks.", style = MaterialTheme.typography.bodySmall)
                            }
                        }
                        state.transfers.firstOrNull { it.direction == "receive" && (it.status == "active" || it.status == "complete") }?.let { transfer ->
                            TransferCard(transfer) { selectedId = transfer.id; model.display(false); model.scan(false) }
                        }
                        Text("Keep this screen open during transfer. Your queue and received chunks are saved automatically.", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                }
            }
        }
    }
}

@Composable
fun SectionCard(title: String, subtitle: String, content: @Composable ColumnScope.() -> Unit) {
    Card(Modifier.fillMaxWidth(), colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainer)) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Text(title, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.Bold)
            Text(subtitle, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            content()
        }
    }
}

@Composable
fun OpticalFrame(frame: QrFrame?, modifier: Modifier = Modifier) {
    Box(modifier.background(Color.White), contentAlignment = Alignment.Center) {
        if (frame == null) Text("Start the display when ready", color = Color(0xFF53635A), style = MaterialTheme.typography.bodyMedium)
        else {
            val bitmap = remember(frame.bitmap) { frame.bitmap.asImageBitmap() }
            Canvas(Modifier.fillMaxSize().semantics { contentDescription = "Optical transfer QR code" }) {
                val scale = floor(minOf(size.width, size.height) / bitmap.width).toInt()
                if (scale > 0) {
                    val side = bitmap.width * scale
                    drawImage(bitmap, srcSize = IntSize(bitmap.width, bitmap.height),
                        dstOffset = IntOffset(((size.width - side) / 2).toInt(), ((size.height - side) / 2).toInt()),
                        dstSize = IntSize(side, side), filterQuality = FilterQuality.None)
                }
            }
        }
    }
}
