package sh.fieldsmith.app

object FieldsmithNativeBridge {
    init {
        System.loadLibrary("fieldsmith_mcp")
    }

    /**
     * Starts the embedded Rust HTTP compute + MCP + static asset server on `127.0.0.1:portHint`
     * (pass `0` for an OS-assigned ephemeral port) and returns the bound TCP port, or `-1` on error.
     */
    @JvmStatic
    external fun startLocalServer(portHint: Int): Int
}
