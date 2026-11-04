package com.wasimaster.wmpass.system;

// Runs in Shizuku's process, as the shell or root. See WifiReader.
interface IWifiReader {
    // Shizuku calls this transaction to stop the service.
    void destroy() = 16777114;

    // Saved networks as a JSON array of {ssid, security, password, hidden}.
    String read() = 1;
}
