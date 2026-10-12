package com.qrtx.transceiver

import android.graphics.Bitmap
import org.json.JSONArray
import org.json.JSONObject

data class TransferFile(
    val id: Int, val name: String, val size: Long, val directory: Boolean,
    val sha256: String, val verified: Boolean, val receivedBytes: Long,
    val receivedChunks: Long, val totalChunks: Long
)
data class Transfer(
    val id: String, val direction: String, val status: String, val files: List<TransferFile>,
    val cycles: Long, val frames: Long, val error: String?, val bytesPerSecond: Double, val created: Long
) {
    val totalBytes get() = files.sumOf { it.size }
    val progress: Float get() = if (totalBytes == 0L) {
        if (status == "complete" || status == "confirmed") 1f else 0f
    } else if (direction == "receive") {
        (files.sumOf { it.receivedBytes }.toDouble() / totalBytes).toFloat().coerceIn(0f, 1f)
    } else {
        val totalChunks = files.sumOf { it.totalChunks }
        if (totalChunks == 0L) 0f else (files.sumOf { it.receivedChunks }.toDouble() / totalChunks).toFloat().coerceIn(0f, 1f)
    }
    val title get() = files.firstOrNull { !it.directory }?.name?.substringAfterLast('/') ?: "Empty folder"
    val statusLabel get() = when (status) {
        "confirmed" -> "Delivery confirmed by trusted peer"
        "complete" -> if (direction == "receive") "Received · SHA-256 verified" else "Complete"
        "acknowledged" -> "Receipt claim · untrusted"
        "displayed" -> "Loop shown · delivery unconfirmed"
        "active" -> if (direction == "receive") "Receiving" else "Queued for display"
        else -> status.replaceFirstChar { it.uppercase() }
    }
}
data class TransferSettings(
    val mode: String = "both", val frameMs: Int = 400, val chunkSize: Int = 400,
    val correction: String = "M", val trustedPeers: List<String> = emptyList()
)
data class QrFrame(val bitmap: Bitmap, val durationMs: Long, val label: String)
data class AppState(
    val ready: Boolean = false, val fingerprint: String = "", val settings: TransferSettings = TransferSettings(),
    val transfers: List<Transfer> = emptyList(), val busy: Boolean = false, val busyLabel: String = "",
    val error: String? = null, val message: String? = null,
    val displaying: Boolean = false, val scanning: Boolean = false, val frontCamera: Boolean = false,
    val frame: QrFrame? = null, val lastScanIssue: String? = null, val accepted: Long = 0, val rejected: Long = 0
)

fun JSONArray.objects(): List<JSONObject> = (0 until length()).map { getJSONObject(it) }
fun JSONArray.strings(): List<String> = (0 until length()).map { getString(it) }
fun parseSnapshot(json: JSONObject, previous: AppState): AppState {
    val profile = json.getJSONObject("profile")
    return previous.copy(
        ready = true,
        fingerprint = json.getString("fingerprint"),
        settings = TransferSettings(
            profile.getString("mode"), profile.getInt("frame_ms"), profile.getInt("chunk_size"),
            profile.getString("correction"), profile.getJSONArray("trusted_peers").strings()
        ),
        transfers = json.getJSONArray("transfers").objects().map { job ->
            Transfer(
                job.getString("id"), job.getString("direction"), job.getString("status"),
                job.getJSONArray("files").objects().map { f ->
                    TransferFile(
                        f.getInt("id"), f.getString("name"), f.getLong("size"), f.getBoolean("directory"),
                        f.getString("sha256"), f.getBoolean("verified"), f.getLong("receivedBytes"),
                        f.getLong("receivedChunks"), f.getLong("totalChunks")
                    )
                }, job.getLong("cycles"), job.getLong("frames"),
                if (job.isNull("error")) null else job.getString("error"),
                job.optDouble("bytesPerSecond", 0.0), job.getLong("created")
            )
        }.sortedByDescending { it.created }
    )
}
fun formatBytes(size: Long): String = when {
    size < 1024 -> "$size B"
    size < 1024 * 1024 -> "%.1f KiB".format(size / 1024.0)
    size < 1024 * 1024 * 1024 -> "%.1f MiB".format(size / (1024.0 * 1024))
    else -> "%.1f GiB".format(size / (1024.0 * 1024 * 1024))
}
