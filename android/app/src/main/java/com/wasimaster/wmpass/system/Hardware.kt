package com.wasimaster.wmpass.system

import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.hardware.usb.UsbConstants
import android.hardware.usb.UsbDevice
import android.hardware.usb.UsbDeviceConnection
import android.hardware.usb.UsbEndpoint
import android.hardware.usb.UsbInterface
import android.hardware.usb.UsbManager
import android.nfc.NfcAdapter
import android.nfc.Tag
import android.nfc.tech.IsoDep
import androidx.core.content.ContextCompat
import androidx.fragment.app.FragmentActivity
import com.wasimaster.wmpass.core.HwDevice
import java.io.IOException
import com.wasimaster.wmpass.core.Engine
import kotlin.coroutines.resume
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitCancellation
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.selects.select
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put

/** A hardware key found on NFC or USB, ready to be spoken to. */
class FoundKey(val device: HwDevice, val nfc: Boolean, val close: () -> Unit)

object HardwareKeys {
    private const val YUBICO = 0x1050
    private const val HID_CLASS = 3
    private const val USB_TIMEOUT = 2000
    private const val PERMISSION = "com.wasimaster.wmpass.USB_PERMISSION"

    private class NfcKey(private val tag: IsoDep) : HwDevice {
        override fun transceive(apdu: ByteArray): ByteArray = tag.transceive(apdu)
    }

    private class FidoUsb(
        private val connection: UsbDeviceConnection,
        private val input: UsbEndpoint,
        private val output: UsbEndpoint,
    ) : HwDevice {
        override fun hidWrite(report: ByteArray): ByteArray {
            if (connection.bulkTransfer(output, report, report.size, USB_TIMEOUT) != report.size) throw IOException("write")
            return ByteArray(0)
        }

        override fun hidRead(ignored: ByteArray): ByteArray {
            val buffer = ByteArray(64)
            if (connection.bulkTransfer(input, buffer, 64, USB_TIMEOUT) != 64) throw IOException("read")
            return buffer
        }
    }

    /** A YubiKey's OTP interface answers through feature reports on the control endpoint. */
    private class OtpUsb(private val connection: UsbDeviceConnection, private val iface: Int) : HwDevice {
        override fun otpGet(ignored: ByteArray): ByteArray {
            val buffer = ByteArray(8)
            // Class request, interface recipient, GET_REPORT, feature report 0.
            if (connection.controlTransfer(0xA1, 0x01, 0x0300, iface, buffer, 8, USB_TIMEOUT) != 8) throw IOException("get")
            return buffer
        }

        override fun otpSet(report: ByteArray): ByteArray {
            if (connection.controlTransfer(0x21, 0x09, 0x0300, iface, report, 8, USB_TIMEOUT) != 8) throw IOException("set")
            return ByteArray(0)
        }
    }

    private fun usbDevice(context: Context): Pair<UsbManager, UsbDevice>? {
        val manager = context.getSystemService(UsbManager::class.java) ?: return null
        val device = manager.deviceList.values.firstOrNull { d ->
            d.vendorId == YUBICO || (0 until d.interfaceCount).any { i ->
                d.getInterface(i).interfaceClass == HID_CLASS && d.getInterface(i).endpointCount >= 2
            }
        } ?: return null
        return manager to device
    }

    private suspend fun permission(context: Context, manager: UsbManager, device: UsbDevice, ask: Boolean): Boolean {
        if (manager.hasPermission(device)) return true
        if (!ask) return false
        return suspendCancellableCoroutine { waiter ->
            val receiver = object : BroadcastReceiver() {
                override fun onReceive(c: Context, intent: Intent) {
                    context.unregisterReceiver(this)
                    if (waiter.isActive) waiter.resume(intent.getBooleanExtra(UsbManager.EXTRA_PERMISSION_GRANTED, false))
                }
            }
            ContextCompat.registerReceiver(context, receiver, IntentFilter(PERMISSION), ContextCompat.RECEIVER_NOT_EXPORTED)
            val intent = Intent(PERMISSION).setPackage(context.packageName)
            manager.requestPermission(device, PendingIntent.getBroadcast(context, 0, intent, PendingIntent.FLAG_MUTABLE))
            waiter.invokeOnCancellation { runCatching { context.unregisterReceiver(receiver) } }
        }
    }

    /** A key plugged in now, for [fido] (raw HID) or for a YubiKey's OTP interface. */
    suspend fun usb(context: Context, fido: Boolean, ask: Boolean = true): FoundKey? {
        val (manager, device) = usbDevice(context) ?: return null
        if (!permission(context, manager, device, ask)) return null
        val connection = manager.openDevice(device) ?: return null
        val interfaces: List<UsbInterface> = (0 until device.interfaceCount).map(device::getInterface).filter { it.interfaceClass == HID_CLASS }
        if (fido) {
            // The FIDO interface is the HID one with interrupt endpoints both ways.
            val chosen = interfaces.firstOrNull { it.endpointCount >= 2 }
            if (chosen == null) {
                connection.close()
                return null
            }
            val endpoints = (0 until chosen.endpointCount).map(chosen::getEndpoint)
            val input = endpoints.firstOrNull { it.direction == UsbConstants.USB_DIR_IN }
            val output = endpoints.firstOrNull { it.direction == UsbConstants.USB_DIR_OUT }
            // Not a key's interface after all, or one another app holds: let go of the device
            // rather than keep it open with nothing to say to it.
            if (input == null || output == null || !connection.claimInterface(chosen, true)) {
                connection.close()
                return null
            }
            return FoundKey(FidoUsb(connection, input, output), false) {
                connection.releaseInterface(chosen)
                connection.close()
            }
        }
        val chosen = interfaces.firstOrNull { it.interfaceSubclass == 1 && it.interfaceProtocol == 1 } ?: interfaces.firstOrNull()
        if (chosen == null) {
            connection.close()
            return null
        }
        if (!connection.claimInterface(chosen, true)) {
            connection.close()
            return null
        }
        return FoundKey(OtpUsb(connection, chosen.id), false) {
            connection.releaseInterface(chosen)
            connection.close()
        }
    }

    /** Waits for a key to be held to the phone. */
    suspend fun nfc(activity: FragmentActivity): FoundKey? {
        val adapter = NfcAdapter.getDefaultAdapter(activity) ?: return null
        if (!adapter.isEnabled) return null
        return suspendCancellableCoroutine { waiter ->
            adapter.enableReaderMode(
                activity,
                { tag: Tag ->
                    val iso = IsoDep.get(tag)
                    if (iso != null && waiter.isActive) {
                        val connected = runCatching {
                            iso.connect()
                            iso.timeout = 30_000
                        }.isSuccess
                        // Gone again before it could be spoken to, as a key brushed past the phone:
                        // keep waiting for a proper tap rather than fail on this one.
                        if (!connected) {
                            runCatching { iso.close() }
                            return@enableReaderMode
                        }
                        waiter.resume(
                            FoundKey(NfcKey(iso), true) {
                                runCatching { iso.close() }
                                runCatching { adapter.disableReaderMode(activity) }
                            },
                        )
                    }
                },
                NfcAdapter.FLAG_READER_NFC_A or NfcAdapter.FLAG_READER_NFC_B or NfcAdapter.FLAG_READER_SKIP_NDEF_CHECK,
                null,
            )
            waiter.invokeOnCancellation { runCatching { adapter.disableReaderMode(activity) } }
        }
    }

    /** Whichever comes first: a key held to the phone, or one plugged in. */
    suspend fun find(activity: FragmentActivity, fido: Boolean): FoundKey = coroutineScope {
        val overNfc = async { nfc(activity) ?: awaitCancellation() }
        val overUsb = async {
            var found: FoundKey? = null
            // A key whose permission was refused is asked about once, not every time round.
            val refused = mutableSetOf<String>()
            while (found == null) {
                val plugged = usbDevice(activity)
                if (plugged != null) {
                    val name = plugged.second.deviceName
                    found = usb(activity, fido, ask = name !in refused)
                    if (found == null && !plugged.first.hasPermission(plugged.second)) refused += name
                }
                if (found == null) delay(600)
            }
            found
        }
        val winner = select<FoundKey> {
            overNfc.onAwait { it }
            overUsb.onAwait { it }
        }
        overNfc.cancel()
        overUsb.cancel()
        winner
    }

    /**
     * Waits for a key and has it answer: for [factor] if there is one, or enrols it if not.
     * Returns the factor as JSON and its output, which the caller wipes.
     */
    suspend fun tap(activity: FragmentActivity, fido: Boolean, factor: String): Pair<String, ByteArray> {
        val key = find(activity, fido)
        try {
            val method = (if (fido) "fido" else "otp") + if (key.nfc) "_nfc" else "_usb"
            val reply = Engine.hardware(method, factor, key.device)
            return reply.json.toString() to reply.secret!!.consume { it.copyOf() }
        } finally {
            key.close()
        }
    }

    /**
     * A YubiKey's answer for a KeePass database it is part of the key to, from the slot
     * KeePassXC set up. The caller wipes it.
     */
    suspend fun keepassAnswer(activity: FragmentActivity, data: ByteArray, slot: Int): ByteArray {
        val reply = Engine.call("kdbx.challenge", blobs = listOf(data))
        val challenge = (reply.json as JsonObject)["challenge"]!!.jsonPrimitive.content
        val factor = buildJsonObject {
            put("kind", "yubikey")
            put("challenge", challenge)
            put("slot", slot)
        }.toString()
        return tap(activity, false, factor).second
    }

    fun nfcAvailable(context: Context) = NfcAdapter.getDefaultAdapter(context)?.isEnabled == true

    fun usbAttached(context: Context) = usbDevice(context) != null
}
