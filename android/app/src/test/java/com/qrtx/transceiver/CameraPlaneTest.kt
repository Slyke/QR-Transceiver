package com.qrtx.transceiver

import java.nio.ByteBuffer
import org.junit.Assert.assertArrayEquals
import org.junit.Test

class CameraPlaneTest {
    @Test fun paddedRowsAndStridedPixels() {
        val bytes = ByteBuffer.wrap(byteArrayOf(99, 1, 0, 2, 0, 3, 0, 88, 88, 4, 0, 5, 0, 6))
        bytes.position(1)
        assertArrayEquals(byteArrayOf(1, 2, 3, 4, 5, 6), copyLuma(bytes, 3, 2, 8, 2))
    }
    @Test(expected = IllegalArgumentException::class) fun rejectsTruncatedPlane() {
        copyLuma(ByteBuffer.wrap(byteArrayOf(1, 2, 3)), 2, 2, 2, 1)
    }
}
