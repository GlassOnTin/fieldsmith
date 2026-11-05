package sh.fieldsmith.app

import android.annotation.SuppressLint
import android.app.Activity
import android.content.Intent
import android.content.pm.ApplicationInfo
import android.graphics.Color
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.system.Os
import android.view.Window
import android.view.WindowInsets
import android.webkit.ValueCallback
import android.webkit.WebChromeClient
import android.webkit.WebSettings
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.FrameLayout
import java.io.File

class MainActivity : Activity() {
    private lateinit var webView: WebView
    private var fileUploadCallback: ValueCallback<Array<Uri>>? = null

    @SuppressLint("SetJavaScriptEnabled")
    override fun onCreate(savedInstanceState: Bundle?) {
        requestWindowFeature(Window.FEATURE_NO_TITLE)
        super.onCreate(savedInstanceState)

        val isDebuggable = (applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE) != 0
        WebView.setWebContentsDebuggingEnabled(isDebuggable)

        val webDir = File(filesDir, "web")
        if (isDebuggable) {
            webDir.deleteRecursively()
        }
        extractBundledAssetsIfNeeded("web", webDir)
        val workspaceFile = File(filesDir, "fieldsmith-workspace.json")

        Os.setenv("FIELDSMITH_WEB_DIR", webDir.absolutePath, true)
        Os.setenv("FIELDSMITH_WORKSPACE", workspaceFile.absolutePath, true)

        // Cap native Rayon / Tokio worker threads on mobile big.LITTLE SoCs (e.g. 8-core
        // phones with 4 efficiency cores) so heavy FEM/VNA solves stay on the performance
        // cores and never starve Android's UI / RenderThread or background apps.
        val totalCores = Runtime.getRuntime().availableProcessors()
        val perfThreads = (totalCores / 2).coerceIn(2, 4)
        Os.setenv("RAYON_NUM_THREADS", perfThreads.toString(), true)
        Os.setenv("TOKIO_WORKER_THREADS", "2", true)

        // Prefer deterministic port 18789 so `adb forward tcp:18789 tcp:18789` connects
        // desktop/proot MCP agents directly to the mobile app's `/mcp` & `/api/mcp` server.
        val boundPort = FieldsmithNativeBridge.startLocalServer(18789).let {
            if (it > 0) it else FieldsmithNativeBridge.startLocalServer(0)
        }

        webView = WebView(this).apply {
            settings.apply {
                javaScriptEnabled = true
                domStorageEnabled = true
                allowFileAccess = false
                allowContentAccess = true
                mediaPlaybackRequiresUserGesture = false
                mixedContentMode = WebSettings.MIXED_CONTENT_COMPATIBILITY_MODE
            }
            webViewClient = WebViewClient()
            webChromeClient = object : WebChromeClient() {
                override fun onShowFileChooser(
                    view: WebView?,
                    filePathCallback: ValueCallback<Array<Uri>>?,
                    fileChooserParams: FileChooserParams?
                ): Boolean {
                    fileUploadCallback?.onReceiveValue(null)
                    fileUploadCallback = filePathCallback
                    val intent = fileChooserParams?.createIntent() ?: Intent(Intent.ACTION_GET_CONTENT).apply {
                        addCategory(Intent.CATEGORY_OPENABLE)
                        type = "*/*"
                    }
                    startActivityForResult(intent, FILE_CHOOSER_REQUEST_CODE)
                    return true
                }
            }
        }

        val rootContainer = FrameLayout(this).apply {
            setBackgroundColor(Color.parseColor("#0f172a"))
            addView(
                webView,
                FrameLayout.LayoutParams(
                    FrameLayout.LayoutParams.MATCH_PARENT,
                    FrameLayout.LayoutParams.MATCH_PARENT
                )
            )
            setOnApplyWindowInsetsListener { view, insets ->
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                    val bars = insets.getInsets(
                        WindowInsets.Type.systemBars() or WindowInsets.Type.displayCutout()
                    )
                    view.setPadding(bars.left, bars.top, bars.right, bars.bottom)
                    WindowInsets.CONSUMED
                } else {
                    @Suppress("DEPRECATION")
                    view.setPadding(
                        insets.systemWindowInsetLeft,
                        insets.systemWindowInsetTop,
                        insets.systemWindowInsetRight,
                        insets.systemWindowInsetBottom
                    )
                    insets
                }
            }
        }

        setContentView(rootContainer)
        if (boundPort > 0) {
            webView.loadUrl("http://127.0.0.1:$boundPort/")
        }
    }

    private fun extractBundledAssetsIfNeeded(assetPrefix: String, destDir: File) {
        val children = assets.list(assetPrefix) ?: return
        if (children.isEmpty()) {
            destDir.parentFile?.mkdirs()
            assets.open(assetPrefix).use { input ->
                destDir.outputStream().use { output ->
                    input.copyTo(output)
                }
            }
        } else {
            destDir.mkdirs()
            for (child in children) {
                extractBundledAssetsIfNeeded("$assetPrefix/$child", File(destDir, child))
            }
        }
    }

    @Deprecated("Deprecated in Java")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode == FILE_CHOOSER_REQUEST_CODE) {
            val results = WebChromeClient.FileChooserParams.parseResult(resultCode, data)
            fileUploadCallback?.onReceiveValue(results)
            fileUploadCallback = null
        }
    }

    companion object {
        private const val FILE_CHOOSER_REQUEST_CODE = 1001
    }
}
