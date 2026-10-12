package com.qrtx.transceiver

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color

@Composable
fun QrTheme(content: @Composable () -> Unit) {
    val colors = if (isSystemInDarkTheme()) darkColorScheme(
        primary = Color(0xFF98E6CA), onPrimary = Color(0xFF003829),
        secondary = Color(0xFFBACCC2), secondaryContainer = Color(0xFF314E41), onSecondaryContainer = Color(0xFFD4E9DC), background = Color(0xFF101916),
        surface = Color(0xFF101916), surfaceContainer = Color(0xFF1B2520),
        surfaceVariant = Color(0xFF3D4942), outline = Color(0xFF87958C)
    ) else lightColorScheme(
        primary = Color(0xFF086B50), onPrimary = Color.White,
        secondary = Color(0xFF4F6358), secondaryContainer = Color(0xFFD5EBDD), onSecondaryContainer = Color(0xFF163F2F),
        outline = Color(0xFF74877B), outlineVariant = Color(0xFFBDCEC3), background = Color(0xFFF4FBF4),
        surface = Color(0xFFF4FBF4), surfaceContainer = Color(0xFFE8F0E8),
        surfaceVariant = Color(0xFFDCE6DC)
    )
    MaterialTheme(colorScheme = colors, content = content)
}
