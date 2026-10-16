package com.qrtx.transceiver

import android.app.Application
import android.graphics.Bitmap
import android.graphics.Color
import android.net.Uri
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import java.io.File
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import org.json.JSONArray
import org.json.JSONObject

class TransferViewModel(application: Application) : AndroidViewModel(application) {
    private val mutableState = MutableStateFlow(AppState())
    val state = mutableState.asStateFlow()
    private val serial = Mutex()
    @Volatile private var engine: NativeEngine? = null
    @Volatile private var foreground = false
    private val documents = Documents(application)
    private var frameGeneration = 0L

    init {
        viewModelScope.launch {
            try {
                engine = withContext(Dispatchers.IO) { NativeEngine(File(application.filesDir, "engine")) }
                refresh()
                while (isActive) {
                    if (foreground) {
                        try { call("maintain"); refresh() }
                        catch (error: Throwable) { report(error) }
                    }
                    delay(1200)
                }
            } catch (error: Throwable) { report(error) }
        }
        viewModelScope.launch {
            while (isActive) {
                if (foreground && state.value.ready && state.value.displaying) {
                    val generation = frameGeneration
                    try {
                        val json = call("next_frame") as? JSONObject
                        val frame = json?.let { withContext(Dispatchers.Default) { render(it) } }
                        if (generation == frameGeneration && foreground && state.value.displaying) {
                            mutableState.update { it.copy(frame = frame) }
                            // Start the dwell after Compose has a chance to present this frame.
                            delay(32 + (frame?.durationMs ?: 250))
                        }
                    } catch (error: Throwable) {
                        mutableState.update { it.copy(displaying = false) }
                        report(error)
                    }
                } else delay(100)
            }
        }
    }

    private suspend fun call(method: String, params: JSONObject = JSONObject()): Any? =
        withContext(Dispatchers.IO) {
            serial.withLock { checkNotNull(engine) { "Engine is starting" }.call(method, params) }
        }
    private suspend fun refresh() {
        val json = call("status") as JSONObject
        mutableState.update { parseSnapshot(json, it) }
    }
    fun foreground(value: Boolean) { foreground = value; frameGeneration++ }
    fun display(value: Boolean) {
        frameGeneration++
        mutableState.update { it.copy(displaying = value, frame = if (value) it.frame else null) }
    }
    fun scan(value: Boolean) { mutableState.update { it.copy(scanning = value) } }
    fun switchCamera() { mutableState.update { it.copy(frontCamera = !it.frontCamera) } }
    fun clearError() { mutableState.update { it.copy(error = null) } }
    fun clearMessage() { mutableState.update { it.copy(message = null) } }
    fun encryptSend(value: Boolean) { mutableState.update { it.copy(encryptSend = value, sendPassword = "") } }
    fun sendPassword(value: String) { mutableState.update { it.copy(sendPassword = value) } }
    fun receivePassword(id: String?, password: String?) {
        task("Updating receive password…") {
            try {
                call("receive_password", JSONObject().put("id", id ?: JSONObject.NULL)
                    .put("password", password ?: JSONObject.NULL))
                mutableState.update { it.copy(message = if (password == null) "Receive password cleared." else "Password applied. Scanning can continue.") }
            } finally { refresh() }
        }
    }
    fun cameraError(message: String) { mutableState.update { it.copy(scanning = false, error = message) } }

    fun analyze(bytes: ByteArray, width: Int, height: Int) {
        if (!foreground || !state.value.scanning) return
        try {
            val result = engine?.scan(bytes, width, height) ?: return
            mutableState.update { it.copy(
                accepted = it.accepted + result.getInt("accepted"),
                rejected = it.rejected + result.getInt("rejected"),
                lastScanIssue = when {
                    result.getInt("accepted") > 0 -> null
                    !result.isNull("lastError") -> result.getString("lastError")
                    else -> it.lastScanIssue
                }
            ) }
        } catch (error: Exception) { cameraError(error.message ?: "Camera processing failed") }
    }

    private fun task(label: String, block: suspend () -> Unit) {
        if (state.value.busy || !state.value.ready) return
        mutableState.update { it.copy(busy = true, busyLabel = label) }
        viewModelScope.launch {
            try { block(); refresh() }
            catch (error: Throwable) { report(error) }
            finally { mutableState.update { it.copy(busy = false, busyLabel = "") } }
        }
    }

    fun importFiles(uris: List<Uri>) {
        if (uris.isEmpty()) return
        task("Importing files…") { enqueue(withContext(Dispatchers.IO) { documents.importFiles(uris) }, false) }
    }
    fun importFolder(uri: Uri) {
        task("Importing folder…") { enqueue(withContext(Dispatchers.IO) { documents.importTree(uri) }, true) }
    }
    private suspend fun enqueue(files: List<File>, recursive: Boolean) {
        try {
            val params = JSONObject().put("paths", JSONArray(files.map { it.absolutePath })).put("recursive", recursive)
            if (state.value.encryptSend) params.put("password", state.value.sendPassword)
            call("enqueue", params)
            mutableState.update { it.copy(message = "Added to the transfer queue. Start the display when ready.") }
        } catch (error: Throwable) {
            withContext(Dispatchers.IO) { documents.discard(files) }
            throw error
        } finally {
            mutableState.update { it.copy(sendPassword = "") }
        }
    }
    fun configure(settings: TransferSettings) {
        task("Saving settings…") {
            display(false)
            scan(false)
            call("configure", JSONObject().put("mode", settings.mode)
                .put("frame_ms", settings.frameMs).put("chunk_size", settings.chunkSize)
                .put("correction", settings.correction).put("trusted_peers", JSONArray(settings.trustedPeers)))
        }
    }
    fun mode(mode: String) = configure(state.value.settings.copy(mode = mode))
    fun action(id: String, action: String) {
        task("Updating transfer…") {
            call("action", JSONObject().put("id", id).put("action", action))
            frameGeneration++
            mutableState.update { it.copy(frame = null) }
        }
    }
    fun requestMissing(id: String, file: Int) {
        task("Requesting missing chunks…") {
            call("missing", JSONObject().put("id", id).put("file", file))
            mutableState.update { it.copy(message = "Request queued. Use Both mode and start the display to send it.") }
        }
    }
    fun export(id: String, file: Int, uri: Uri) {
        task("Saving verified file…") {
            val json = call("export", JSONObject().put("id", id).put("file", file)) as JSONObject
            withContext(Dispatchers.IO) { documents.export(File(json.getString("path")), uri) }
            mutableState.update { it.copy(message = "Verified file saved.") }
        }
    }
    fun exportFolder(id: String, uri: Uri) {
        task("Saving verified folder…") {
            val files = call("export_batch", JSONObject().put("id", id)) as JSONArray
            withContext(Dispatchers.IO) { documents.exportBatch(files, uri, "QRTX-${id.take(8)}") }
            mutableState.update { it.copy(message = "Verified transfer saved in its own folder.") }
        }
    }
    suspend fun payload(id: String, file: Int, offset: Long): JSONObject =
        call("payload", JSONObject().put("id", id).put("file", file).put("offset", offset)) as JSONObject

    private fun report(error: Throwable) {
        if (error is CancellationException) throw error
        mutableState.update { it.copy(error = error.message ?: "Something went wrong") }
    }
    private fun render(json: JSONObject): QrFrame {
        val width = json.getInt("width")
        val quiet = json.getInt("quietZone")
        val side = width + quiet * 2
        val modules = json.getJSONArray("modules")
        require(width in 21..177 && modules.length() == width * width)
        val pixels = IntArray(side * side) { Color.WHITE }
        for (y in 0 until width) for (x in 0 until width) {
            if (modules.getBoolean(y * width + x)) pixels[(y + quiet) * side + x + quiet] = Color.BLACK
        }
        return QrFrame(
            Bitmap.createBitmap(pixels, side, side, Bitmap.Config.ARGB_8888),
            json.getLong("durationMs"),
            "${json.getString("kind")} · ${json.getInt("index") + 1}/${json.getInt("total")} · loop ${json.getLong("cycle") + 1}"
        )
    }
    override fun onCleared() { engine?.close(); engine = null; super.onCleared() }
}
