package com.qrtx.transceiver

import android.graphics.Bitmap
import android.net.Uri
import java.io.File
import androidx.lifecycle.ViewModelProvider
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import org.junit.Rule
import org.junit.Test

class AppSmokeTest {
    @get:Rule val compose = createAndroidComposeRule<MainActivity>()
    @Test fun phoneNavigationAndSettings() {
        compose.waitUntil(30_000) { compose.onAllNodesWithText("Transfer files").fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithText("Receive").performClick()
        compose.onNodeWithText("Start camera").assertExists()
        compose.onNodeWithText("Files", useUnmergedTree = true).performClick()
        compose.onNodeWithText("Your files").assertExists()
        compose.onNodeWithText("Settings", useUnmergedTree = true).performClick()
        compose.onNodeWithText("Transfer settings").assertExists()
        compose.onNodeWithText("Frame duration (ms)").performTextReplacement("550")
        compose.onNodeWithText("Save settings").performScrollTo().performClick()
        compose.onNodeWithText("Transfer", useUnmergedTree = true).performClick()
        compose.onNodeWithText("Both").performClick()
        compose.waitUntil(15_000) { compose.onAllNodesWithText("Start display").fetchSemanticsNodes().isNotEmpty() }
        val model = ViewModelProvider(compose.activity)[TransferViewModel::class.java]
        compose.waitUntil(15_000) { !model.state.value.busy }
        val count = model.state.value.transfers.size
        val file = File(compose.activity.cacheDir, "preview.txt").apply { writeText("QR Transceiver on Android") }
        compose.runOnUiThread { model.importFiles(listOf(Uri.fromFile(file))) }
        compose.waitUntil(20_000) { model.state.value.transfers.size > count && !model.state.value.busy }
        compose.waitUntil(20_000) { model.state.value.message == null }
        compose.onNodeWithText("Start display").performScrollTo().performClick()
        try { compose.waitUntil(5_000) { model.state.value.displaying } }
        catch (error: Throwable) { throw AssertionError("Start-display state: ${model.state.value}; lifecycle=${compose.activity.lifecycle.currentState}", error) }
        compose.onNodeWithText("Full screen").performScrollTo().performClick()
        try {
            compose.waitUntil(20_000) { compose.onAllNodesWithContentDescription("Optical transfer QR code").fetchSemanticsNodes().isNotEmpty() }
        } catch (error: Throwable) {
            throw AssertionError("QR display state: ${model.state.value}; lifecycle=${compose.activity.lifecycle.currentState}; tree=${compose.onRoot().printToString()}", error)
        }
        org.junit.Assert.assertNull("Unexpected app error", model.state.value.error)
        capture("android-display.png")
        compose.onNodeWithText("Pause display").performClick()
        compose.onNodeWithText("Exit full screen").performClick()
        compose.onNodeWithText("Transfer files").performScrollTo()
        capture("android-home.png")
    }
    private fun capture(name: String) {
        File(compose.activity.cacheDir, name).outputStream().use {
            compose.onAllNodes(isRoot())[0].captureToImage().asAndroidBitmap().compress(Bitmap.CompressFormat.PNG, 100, it)
        }
    }
}
