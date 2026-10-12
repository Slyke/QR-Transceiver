package com.qrtx.transceiver

import java.io.Closeable
import java.io.File
import java.util.concurrent.atomic.AtomicLong
import org.json.JSONObject

class NativeEngine(root: File) : Closeable {
    companion object { init { System.loadLibrary("qrtx_android") } }

    private external fun nativeOpen(root: String): String
    private external fun nativeCall(handle: Long, method: String, params: String): String
    private external fun nativeScan(handle: Long, bytes: ByteArray, width: Int, height: Int): String
    private external fun nativeClose(handle: Long)

    private val handle = AtomicLong((unwrap(nativeOpen(root.absolutePath)) as Number).toLong())
    fun call(method: String, params: JSONObject = JSONObject()): Any? =
        unwrap(nativeCall(handle.get(), method, params.toString()))

    fun scan(bytes: ByteArray, width: Int, height: Int): JSONObject =
        unwrap(nativeScan(handle.get(), bytes, width, height)) as JSONObject

    override fun close() { val id = handle.getAndSet(0); if (id != 0L) nativeClose(id) }

    private fun unwrap(response: String): Any? {
        val json = JSONObject(response)
        check(json.getBoolean("ok")) { json.optString("error", "Native engine failed") }
        return json.opt("result").takeUnless { it == JSONObject.NULL }
    }
}
