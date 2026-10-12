package com.qrtx.transceiver

import android.util.Size
import android.view.MotionEvent
import androidx.camera.core.CameraSelector
import androidx.camera.core.FocusMeteringAction
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.Preview
import androidx.camera.core.resolutionselector.ResolutionSelector
import androidx.camera.core.resolutionselector.ResolutionStrategy
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.LocalLifecycleOwner
import java.nio.ByteBuffer
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit

/** Copies the Y plane respecting row/pixel stride. QR detection handles rotation itself. */
fun copyLuma(buffer: ByteBuffer, width: Int, height: Int, rowStride: Int, pixelStride: Int): ByteArray {
    require(width > 0 && height > 0 && width.toLong() * height <= 8_847_360L)
    require(rowStride >= (width - 1) * pixelStride + 1 && pixelStride > 0)
    val plane = buffer.duplicate()
    val base = plane.position()
    require(base.toLong() + (height - 1L) * rowStride + (width - 1L) * pixelStride < plane.limit())
    return ByteArray(width * height).also { packed ->
        for (y in 0 until height) for (x in 0 until width) {
            packed[y * width + x] = plane.get(base + y * rowStride + x * pixelStride)
        }
    }
}

@Composable
fun CameraPreview(
    front: Boolean, modifier: Modifier = Modifier,
    onFrame: (ByteArray, Int, Int) -> Unit, onError: (String) -> Unit
) {
    val context = LocalContext.current
    val lifecycle = LocalLifecycleOwner.current
    val frameCallback = rememberUpdatedState(onFrame)
    val errorCallback = rememberUpdatedState(onError)
    val previewView = remember { PreviewView(context).apply {
        implementationMode = PreviewView.ImplementationMode.COMPATIBLE
        scaleType = PreviewView.ScaleType.FIT_CENTER
    } }
    AndroidView(factory = { previewView }, modifier = modifier)
    DisposableEffect(lifecycle, front) {
        val executor = Executors.newSingleThreadExecutor()
        val future = ProcessCameraProvider.getInstance(context)
        var disposed = false
        var provider: ProcessCameraProvider? = null
        val preview = Preview.Builder().build()
        val analysis = ImageAnalysis.Builder()
            .setResolutionSelector(ResolutionSelector.Builder()
                .setResolutionStrategy(ResolutionStrategy(Size(1280, 720), ResolutionStrategy.FALLBACK_RULE_CLOSEST_LOWER_THEN_HIGHER))
                .build())
            .setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST)
            .build()
        future.addListener({
            if (!disposed) {
                try {
                    val cameraProvider = future.get()
                    provider = cameraProvider
                    val selector = if (front) CameraSelector.DEFAULT_FRONT_CAMERA else CameraSelector.DEFAULT_BACK_CAMERA
                    check(cameraProvider.hasCamera(selector)) { "This device has no ${if (front) "front" else "rear"} camera. Try the other camera." }
                    preview.setSurfaceProvider(previewView.surfaceProvider)
                    analysis.setAnalyzer(executor) { frame ->
                        try {
                            val plane = frame.planes[0]
                            val packed = copyLuma(plane.buffer, frame.width, frame.height, plane.rowStride, plane.pixelStride)
                            frameCallback.value(packed, frame.width, frame.height)
                        } catch (error: Exception) {
                            errorCallback.value(error.message ?: "Camera frame could not be read")
                        } finally { frame.close() }
                    }
                    val camera = cameraProvider.bindToLifecycle(lifecycle, selector, preview, analysis)
                    previewView.setOnTouchListener { view, event ->
                        if (event.action == MotionEvent.ACTION_UP) {
                            val point = previewView.meteringPointFactory.createPoint(event.x, event.y)
                            camera.cameraControl.startFocusAndMetering(
                                FocusMeteringAction.Builder(point).setAutoCancelDuration(3, TimeUnit.SECONDS).build()
                            )
                            view.performClick()
                        }
                        true
                    }
                } catch (error: Exception) {
                    errorCallback.value(error.message ?: "Camera could not start")
                }
            }
        }, ContextCompat.getMainExecutor(context))
        onDispose {
            disposed = true
            analysis.clearAnalyzer()
            provider?.unbind(preview, analysis)
            previewView.setOnTouchListener(null)
            executor.shutdown()
        }
    }
}
