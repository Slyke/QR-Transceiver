package com.qrtx.transceiver

import androidx.test.platform.app.InstrumentationRegistry
import java.io.File
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class NativeEngineTest {
    @Test fun nativeTransferRestartAndTrustedReceipt() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val sandbox = File(context.cacheDir, "native-test-${System.nanoTime()}").apply { mkdirs() }
        val senderRoot = File(sandbox, "sender/engine")
        val receiverRoot = File(sandbox, "receiver/engine")
        var sender = NativeEngine(senderRoot)
        var receiver = NativeEngine(receiverRoot)
        try {
            val fingerprint = (receiver.call("status") as JSONObject).getString("fingerprint")
            sender.call("configure", JSONObject().put("trusted_peers", JSONArray(listOf(fingerprint))))
            val source = File(sandbox, "sender/imports/test/data.bin")
            source.parentFile!!.mkdirs()
            val original = ByteArray(8192) { (it % 251).toByte() }
            source.writeBytes(original)
            val id = (sender.call("enqueue", JSONObject().put("paths", JSONArray(listOf(source.path)))) as JSONObject).getString("id")
            repeat(12) { index ->
                val frame = sender.call("next_frame") as? JSONObject
                if (frame != null && index % 3 != 0) runCatching { receiver.call("ingest", JSONObject().put("text", frame.getString("text"))) }
            }
            receiver.close()
            receiver = NativeEngine(receiverRoot)
            val exportParams = JSONObject().put("id", id).put("file", 1)
            var receivedPath: String? = null
            var lastScan = JSONObject()
            for (index in 0 until 80) {
                val frame = sender.call("next_frame") as? JSONObject
                if (frame != null) {
                    val width = frame.getInt("width")
                    val modules = frame.getJSONArray("modules")
                    val side = (width + 8) * 4
                    val raster = ByteArray(side * side) { 0xff.toByte() }
                    for (y in 0 until width) for (x in 0 until width) if (modules.getBoolean(y * width + x)) {
                        for (yy in 0 until 4) for (xx in 0 until 4) raster[((y + 4) * 4 + yy) * side + (x + 4) * 4 + xx] = 0
                    }
                    lastScan = receiver.scan(raster, side, side)
                    if (index % 10 == 0) android.util.Log.i("QRTXTest", "Raster $index: $lastScan")
                }
                receivedPath = runCatching { (receiver.call("export", exportParams) as JSONObject).getString("path") }.getOrNull()
                if (receivedPath != null) break
            }
            assertNotNull("Receive must finish; lastScan=$lastScan; state=${receiver.call("status")}", receivedPath)
            assertArrayEquals(original, File(receivedPath!!).readBytes())
            sender.close()
            sender = NativeEngine(senderRoot)
            var confirmed = false
            for (index in 0 until 60) {
                val receipt = receiver.call("next_frame") as? JSONObject
                if (receipt != null) runCatching { sender.call("ingest", JSONObject().put("text", receipt.getString("text"))) }
                if ((sender.call("status") as JSONObject).getJSONArray("transfers").getJSONObject(0).getString("status") == "confirmed") { confirmed = true; break }
            }
            assertTrue("Trusted receipt confirms delivery", confirmed)
            receiver.call("action", JSONObject().put("id", id).put("action", "remove"))
            assertFalse(File(receivedPath).exists())
        } finally { sender.close(); receiver.close(); sandbox.deleteRecursively() }
    }
}
