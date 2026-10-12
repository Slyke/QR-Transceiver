package com.qrtx.transceiver

import android.content.Context
import android.net.Uri
import android.os.StatFs
import android.provider.OpenableColumns
import androidx.documentfile.provider.DocumentFile
import java.io.File
import java.util.UUID
import java.util.Locale
import org.json.JSONArray

/** Copies provider streams to private, immutable inputs; never resolves content:// to filesystem paths. */
class Documents(private val context: Context) {
    private val imports = File(context.filesDir, "imports").apply { mkdirs() }

    fun importFiles(uris: List<Uri>): List<File> = batch { directory ->
        require(uris.isNotEmpty() && uris.size <= 4096) { "Select between 1 and 4096 files" }
        uris.map { uri ->
            val name = context.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use {
                if (it.moveToFirst()) it.getString(0) else null
            } ?: "document"
            copy(uri, uniqueFile(directory, safeName(name)))
        }
    }

    fun importTree(uri: Uri): List<File> = batch { directory ->
        val tree = requireNotNull(DocumentFile.fromTreeUri(context, uri)) { "Folder is unavailable" }
        var count = 0
        val visited = mutableSetOf<String>()
        fun visit(document: DocumentFile, parent: File, depth: Int): File {
            require(depth <= 32 && ++count <= 4096) { "Folder exceeds 4096 entries or 32 levels" }
            require(visited.add(document.uri.toString())) { "Folder contains a repeated document" }
            val output = uniqueFile(parent, safeName(document.name ?: "document"))
            require(output.relativeTo(directory).path.toByteArray(Charsets.UTF_8).size <= 1000) { "Folder path is too long for transfer" }
            if (document.isDirectory) {
                check(output.mkdir()) { "Could not create imported folder" }
                document.listFiles().forEach { visit(it, output, depth + 1) }
            } else {
                require(document.isFile) { "Unsupported document type" }
                copy(document.uri, output)
            }
            return output
        }
        listOf(visit(tree, directory, 0))
    }

    private fun batch(action: (File) -> List<File>): List<File> {
        val directory = File(imports, UUID.randomUUID().toString())
        check(directory.mkdir()) { "Unable to create import folder" }
        return try { action(directory) } catch (error: Throwable) {
            directory.deleteRecursively()
            throw error
        }
    }

    fun discard(files: List<File>) {
        files.mapNotNull { it.parentFile }.distinct().forEach {
            if (it.parentFile?.canonicalFile == imports.canonicalFile) it.deleteRecursively()
        }
    }

    private fun copy(uri: Uri, target: File): File {
        context.contentResolver.openInputStream(uri).use { input ->
            requireNotNull(input) { "Document provider could not open the file" }
            target.outputStream().use { output ->
                val buffer = ByteArray(128 * 1024)
                var count: Int
                var written = 0L
                while (input.read(buffer).also { count = it } != -1) {
                    written += count
                    require(written <= 16L * 1024 * 1024 * 1024) { "File exceeds 16 GiB" }
                    check(StatFs(imports.path).availableBytes > count + 32L * 1024 * 1024) { "Not enough storage to import this file" }
                    output.write(buffer, 0, count)
                }
                output.fd.sync()
            }
        }
        return target
    }

    fun export(source: File, uri: Uri) {
        val receiveRoot = File(context.filesDir, "engine/received").canonicalFile
        require(source.canonicalPath.startsWith(receiveRoot.path + File.separator)) { "Invalid received file" }
        context.contentResolver.openOutputStream(uri, "wt").use { output ->
            requireNotNull(output) { "Destination could not be opened" }
            source.inputStream().use { it.copyTo(output) }
            output.flush()
        }
    }

    fun exportBatch(files: JSONArray, uri: Uri, folderName: String) {
        val parent = requireNotNull(DocumentFile.fromTreeUri(context, uri)) { "Destination folder is unavailable" }
        require(parent.isDirectory && parent.canWrite()) { "Destination folder is not writable" }
        val existing = parent.listFiles().mapNotNull { it.name?.lowercase(Locale.ROOT) }.toSet()
        var name = folderName
        var suffix = 2
        while (name.lowercase(Locale.ROOT) in existing) name = "$folderName (${suffix++})"
        val output = requireNotNull(parent.createDirectory(name)) { "Could not create export folder" }
        val directories = mutableMapOf("" to output)
        fun directory(parts: List<String>): DocumentFile {
            var key = ""
            var current = output
            for (part in parts) {
                key = if (key.isEmpty()) part else "$key/$part"
                current = directories.getOrPut(key) {
                    requireNotNull(current.createDirectory(part)) { "Could not create folder $key" }
                }
            }
            return current
        }
        for (entry in files.objects()) {
            val parts = entry.getString("name").split('/')
            require(parts.all { it.isNotBlank() && it != "." && it != ".." }) { "Invalid transfer path" }
            if (entry.getBoolean("directory")) directory(parts)
            else {
                val target = requireNotNull(directory(parts.dropLast(1)).createFile("application/octet-stream", parts.last())) {
                    "Could not create exported file"
                }
                export(File(entry.getString("path")), target.uri)
            }
        }
    }

    companion object {
        fun safeName(name: String): String {
            var clean = name.replace(Regex("[\\\\/:*?\"<>|\\p{Cntrl}]"), "_")
                .trim().trimEnd('.', ' ').take(120).ifBlank { "document" }
            var end = clean.length
            while (clean.substring(0, end).toByteArray(Charsets.UTF_8).size > 120) end--
            if (end > 0 && Character.isHighSurrogate(clean[end - 1])) end--
            clean = clean.substring(0, end).trimEnd('.', ' ').ifBlank { "document" }
            val reserved = Regex("(?i)^(CON|PRN|AUX|NUL|COM[0-9]|LPT[0-9])(?:\\..*)?$")
            return if (reserved.matches(clean) || clean.startsWith(".qrtx") || clean == "." || clean == "..") "_$clean" else clean
        }
        fun uniqueFile(parent: File, name: String): File {
            var result = File(parent, name)
            var index = 2
            val existing = parent.list().orEmpty().map { it.lowercase(Locale.ROOT) }.toSet()
            while (result.name.lowercase(Locale.ROOT) in existing) {
                val dot = name.lastIndexOf('.')
                val stem = if (dot > 0) name.substring(0, dot) else name
                val suffix = if (dot > 0) name.substring(dot) else ""
                result = File(parent, "$stem (${index++})$suffix")
            }
            return result
        }
    }
}
