package com.qrtx.transceiver

import org.junit.Assert.*
import org.junit.Test
import java.nio.file.Files

class DocumentNamesTest {
    @Test fun providerNamesCannotEscapeOrUseReservedNames() {
        assertEquals(".._secret", Documents.safeName("../secret"))
        assertEquals("document", Documents.safeName("..."))
        assertEquals("_CON.txt", Documents.safeName("CON.txt"))
        assertEquals("_COM0.txt", Documents.safeName("COM0.txt"))
        assertEquals("_.qrtx-cache", Documents.safeName(".qrtx-cache"))
        assertTrue(Documents.safeName("😊".repeat(120)).toByteArray(Charsets.UTF_8).size <= 120)
        assertEquals("a_b_c", Documents.safeName("a/b\\c"))
        assertTrue(Documents.safeName("x".repeat(200)).length <= 120)
    }
    @Test fun preservesExtensionWhenNamesCollide() {
        val directory = Files.createTempDirectory("qrtx-names").toFile()
        try {
            java.io.File(directory, "a.bin").writeBytes(byteArrayOf(1))
            assertEquals("A (2).bin", Documents.uniqueFile(directory, "A.bin").name)
        } finally { directory.deleteRecursively() }
    }
}
