package com.wasimaster.wmpass.system

import com.wasimaster.wmkeyboard.core.settings.ICloudConfig
import com.wasimaster.wmkeyboard.core.settings.MegaConfig
import com.wasimaster.wmkeyboard.core.settings.NasConfig
import com.wasimaster.wmkeyboard.core.settings.NfsConfig
import com.wasimaster.wmkeyboard.core.settings.PCloudConfig
import com.wasimaster.wmkeyboard.core.settings.PCloudRegion
import com.wasimaster.wmkeyboard.core.settings.ProtonConfig
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Test

class PlaceWireTest {
    private fun roundTrip(place: Destination): Destination = PlaceWire.decode(PlaceWire.encode(place)!!, "new-id")!!

    @Test
    fun aServerPlaceCrossesWithItsLogin() {
        val sftp = Destination(
            id = "old",
            kind = "sftp",
            label = "Home server",
            host = "nas.lan",
            port = 2222,
            user = "me",
            privateKey = "-----BEGIN OPENSSH PRIVATE KEY-----",
            keyPassphrase = "pass",
            path = "/backups",
            hostKey = "SHA256:abc",
        )
        assertEquals(sftp.copy(id = "new-id"), roundTrip(sftp))
        val nfs = Destination(id = "old", kind = "nfs", label = "NFS", path = "wm", nfs = NfsConfig("nfs.lan", "/srv", "wm", 1001, 1002, 2049, 0))
        assertEquals(nfs.copy(id = "new-id"), roundTrip(nfs))
    }

    @Test
    fun noSignInEverCrosses() {
        val places = listOf(
            Destination(id = "1", kind = "dropbox", label = "D", refreshToken = "SECRET-dropbox"),
            Destination(id = "2", kind = "box", label = "B", refreshToken = "SECRET-box"),
            Destination(id = "3", kind = "proton", label = "P", user = "me", proton = ProtonConfig("me", "SECRET-uid", "SECRET-a", "SECRET-r", "SECRET-k")),
            Destination(id = "4", kind = "pcloud", label = "C", user = "me@x", pcloud = PCloudConfig(PCloudRegion.EU, "me@x", "SECRET-auth")),
            Destination(id = "5", kind = "mega", label = "M", user = "me@x", mega = MegaConfig("me@x", "SECRET-sid", "SECRET-key")),
            Destination(
                id = "6",
                kind = "icloud",
                label = "I",
                user = "me@icloud.com",
                icloud = ICloudConfig("me@icloud.com", "app-pw", "SECRET-trust", "SECRET-cookies", "SECRET-client"),
            ),
            Destination(id = "7", kind = "qnap", label = "Q", nas = NasConfig("nas.lan", "me", "pw", "/Public", "SECRET-qtoken", true, "pin")),
            Destination(id = "8", kind = "folder", label = "F", uri = "content://SECRET-tree"),
        )
        for (place in places) {
            val raw = PlaceWire.encode(place)!!
            assertFalse(raw, raw.contains("SECRET"))
        }
        val icloud = roundTrip(places[5])
        assertEquals("me@icloud.com", icloud.user)
        assertEquals("app-pw", icloud.password)
        assertEquals(PCloudRegion.EU, roundTrip(places[3]).pcloud.region)
        val qnap = roundTrip(places[6])
        assertEquals("pw", qnap.nas.password)
        assertEquals("pin", qnap.nas.certPin)
        assertEquals("", qnap.nas.deviceToken)
        assertEquals("", roundTrip(places[7]).uri)
    }

    @Test
    fun eachAppsOwnDefaultFolderStaysItsOwn() {
        val box = Destination(id = "1", kind = "box", label = "Box", folder = Destination.DEFAULT_FOLDER)
        assertEquals("", roundTrip(box).folder)
        // WM Keyboard sends no folder for its own default: here that is WM Pass's default.
        val fromKeyboard = """{"v":1,"kind":"imap","host":"imap.x","user":"me","password":"pw","security":"starttls","folder":""}"""
        val mail = PlaceWire.decode(fromKeyboard, "id")!!
        assertEquals("", mail.folder)
        assertEquals("starttls", mail.security)
        assertEquals(143, mail.port)
        val custom = Destination(id = "1", kind = "mega", label = "M", folder = "Vault copies")
        assertEquals("Vault copies", roundTrip(custom).folder)
    }

    @Test
    fun whatCannotBeAPlaceHereIsRefused() {
        assertNull(PlaceWire.decode("""{"v":1,"kind":"nearby"}""", "id"))
        assertNull(PlaceWire.decode("not json", "id"))
        assertNull(PlaceWire.encode(Destination(id = "1", kind = "carrier-pigeon", label = "?")))
    }
}
