package com.wasimaster.wmpass.ui.help

import android.content.Context
import android.content.res.AssetManager
import com.wasimaster.wmpass.app.Prefs
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.intOrNull

// The guide's words, as scripts/build-help.mjs writes them for the phone: every topic already as
// Android shows it, with the text split into blocks and runs, so nothing here parses markup.

/** One piece of a line: plain words, a name on screen, emphasis, a link, or a key chord. */
@Serializable
data class HelpRun(val t: String, val s: String? = null, val id: String? = null, val action: String? = null)

/** A paragraph of runs, or a list whose items are runs. */
@Serializable
data class HelpBlock(val t: String, val runs: List<HelpRun> = emptyList(), val items: List<List<HelpRun>> = emptyList())

/** One value a setting can take, and what happens when it does. */
@Serializable
data class HelpEffect(val value: JsonPrimitive, val label: String, val text: List<HelpBlock> = emptyList())

@Serializable
data class HelpTopic(
    val id: String,
    val kind: String,
    val title: String,
    val page: String? = null,
    val pref: String? = null,
    val what: List<HelpBlock> = emptyList(),
    val why: List<HelpBlock>? = null,
    val how: List<HelpBlock>? = null,
    @SerialName("when") val whenTo: List<HelpBlock>? = null,
    val effects: List<HelpEffect>? = null,
    val caveats: List<HelpBlock>? = null,
    val figure: String? = null,
    val preview: String? = null,
    val tour: String? = null,
    val related: List<String> = emptyList(),
    val docs: String? = null,
    val words: String? = null,
    /** The first sentence or two as plain text, for the settings kit's own "?" dialog. */
    val plain: String = "",
)

@Serializable
data class HelpBefore(val go: String? = null, val open: String? = null)

@Serializable
data class HelpStep(
    val anchor: String,
    val title: String,
    val body: List<HelpBlock> = emptyList(),
    val placement: String? = null,
    val before: HelpBefore? = null,
    val tryIt: Boolean = false,
)

@Serializable
data class HelpTour(
    val id: String,
    val title: String,
    val intro: List<HelpBlock>? = null,
    val width: String? = null,
    val steps: List<HelpStep> = emptyList(),
)

@Serializable
data class FigureBox(val id: String, val label: String, val sub: String? = null, val icon: String? = null, val tone: String? = null)

@Serializable
data class FigureEdge(val from: String, val to: String, val label: String? = null, val dashed: Boolean = false)

@Serializable
data class HelpFigure(val id: String, val caption: String, val rows: List<List<FigureBox>> = emptyList(), val edges: List<FigureEdge> = emptyList())

@Serializable
data class HelpFile(
    val format: String = "",
    val version: Int = 0,
    val shortcuts: Map<String, String> = emptyMap(),
    val topics: List<HelpTopic> = emptyList(),
    val tours: List<HelpTour> = emptyList(),
    val figures: List<HelpFigure> = emptyList(),
)

/**
 * Everything the guide knows, read from the app's assets the first time something asks. The file
 * is about 300 KB, so the vault warms it on a background thread as it opens rather than paying
 * for it on the first settings page.
 */
object Help {
    private val json = Json { ignoreUnknownKeys = true }

    @Volatile private var assets: AssetManager? = null

    /** Remembers where to read from. Cheap: nothing is read until a topic is asked for. */
    fun attach(context: Context) {
        if (assets == null) assets = context.applicationContext.assets
    }

    /** Parses [text] in place of the asset, for tests. */
    fun parse(text: String): HelpFile = json.decodeFromString(HelpFile.serializer(), text)

    /** The file and its lookups, built together once. */
    private class Index(val file: HelpFile) {
        val byId: Map<String, HelpTopic> = file.topics.associateBy { it.id }

        /** Rows are found by their title on their page, as the content's own check keeps them unique. */
        val byTitle: Map<String, HelpTopic> =
            file.topics.filter { it.kind != "page" && it.kind != "screen" && it.page != null }.associateBy { "${it.page}/${it.title}" }
    }

    private val empty = Index(HelpFile())

    @Volatile private var index: Index? = null

    private val current: Index
        get() = index ?: synchronized(this) {
            index ?: run {
                // Asked before anything said where the assets are: answer with nothing, and read the
                // file properly once something has.
                val from = assets ?: return@run empty
                // A build without the asset, or a file this version cannot read, is a guide with
                // nothing in it: every "?" simply does not draw.
                val read = runCatching { from.open("help.json").bufferedReader().use { parse(it.readText()) } }.getOrDefault(HelpFile())
                Index(read).also { index = it }
            }
        }

    val file: HelpFile get() = current.file

    val topics: List<HelpTopic> get() = file.topics

    fun topic(id: String): HelpTopic? = current.byId[id]

    /** Whether the phone has a settings page [page] to open, at `settings/<page>`. */
    fun hasPage(page: String): Boolean = page in PageNames

    /** The topic explaining one setting, by its key in [Prefs]. */
    fun forPref(key: String): HelpTopic? = topic("setting.$key")

    /** The topic for a row titled [title] on the settings page at [route], such as "settings/security". */
    fun forTitle(route: String?, title: String): HelpTopic? {
        val page = route?.removePrefix("settings/")?.substringBefore('/') ?: return null
        return current.byTitle["$page/$title"]
    }

    /** What a settings row's "?" says when the guide's own sheet is not there to open. */
    fun info(id: String): String? = topic(id)?.plain?.takeIf { it.isNotEmpty() }

    /** A page's topic, by its route. */
    fun forRoute(route: String?): HelpTopic? = route?.takeIf { it.startsWith("settings/") }?.let { topic("page.${it.removePrefix("settings/")}") }

    fun tour(id: String): HelpTour? = file.tours.firstOrNull { it.id == id }

    fun figure(id: String): HelpFigure? = file.figures.firstOrNull { it.id == id }

    /** One line for an option in a choice sheet: what choosing it does. */
    fun effectDetail(key: String, value: Any?): String? =
        forPref(key)?.effects?.firstOrNull { matches(it.value, value) }?.let { plain(it.text) }

    /**
     * Topics whose words hold every word of [query]: titles that start with it first, then titles
     * that hold it, then the rest, each in alphabetical order.
     */
    fun search(query: String): List<HelpTopic> {
        val q = query.trim().lowercase()
        if (q.isEmpty()) return emptyList()
        val words = q.split(' ').filter { it.isNotEmpty() }
        return file.topics.mapNotNull { topic ->
            val title = topic.title.lowercase()
            val text = "$title ${topic.words.orEmpty()} ${topic.plain}".lowercase()
            if (!words.all { it in text }) return@mapNotNull null
            val rank = if (title.startsWith(q)) 0 else if (q in title) 1 else 2
            rank to topic
        }.sortedWith(compareBy({ it.first }, { it.second.title.lowercase() })).map { it.second }
    }

    /** Where a topic lives, the way settings search writes it: "Settings, Appearance, Codes". */
    fun where(topic: HelpTopic): String {
        if (topic.kind == "screen") return "Screens"
        val page = topic.page ?: return "How WM Pass works"
        // The hubs above the page, outermost first, then the page itself unless this is its own topic.
        val path = generateSequence(PageParents[page]) { PageParents[it] }.toList().asReversed().mapNotNull { PageNames[it] }
        val own = if (topic.kind == "page") null else PageNames[page] ?: topic("page.$page")?.title
        return (listOf("Settings") + path + listOfNotNull(own)).joinToString(", ")
    }

    /** A key chord as the person has set it here, or the one the guide knows, or nothing. */
    fun chord(prefs: Prefs?, action: String): String? {
        prefs?.shortcuts?.let { mine -> if (action in mine) return mine[action] }
        return file.shortcuts[action]?.replace("Mod+", "Ctrl+")
    }

    /** The words of [blocks] alone, one line per paragraph or item. */
    fun plain(blocks: List<HelpBlock>?, prefs: Prefs? = null): String =
        blocks.orEmpty().flatMap { block -> if (block.t == "ul") block.items else listOf(block.runs) }
            .joinToString("\n") { runs -> runs.joinToString("") { run -> runText(run, prefs) } }

    internal fun runText(run: HelpRun, prefs: Prefs?): String = when (run.t) {
        "link" -> run.s ?: run.id?.let { topic(it)?.title } ?: ""
        "key" -> run.action?.let { chord(prefs, it) } ?: ""
        else -> run.s.orEmpty()
    }

    /** Whether a value from the guide and one from [Prefs] are the same setting. */
    fun matches(guide: JsonPrimitive, value: Any?): Boolean = when (value) {
        is Boolean -> guide.booleanOrNull == value
        is Int -> guide.intOrNull == value
        is String -> guide.isString && guide.content == value
        else -> false
    }

    /** The settings pages the breadcrumb names, by id, as their screens are titled on the phone. */
    private val PageNames = mapOf(
        "security" to "Security", "shelf" to "Hidden shelf", "breaches" to "Breaches", "health" to "Health checks",
        "folders" to "Folders", "trash" to "Trash", "import" to "Import", "export" to "Export",
        "backup" to "Backups", "sync" to "Sync and accounts", "vaults" to "Nested and shared vaults", "receive" to "Receive an item",
        "hardware" to "Hardware key", "tools" to "Tools", "aliases" to "Email aliases", "inbox" to "Disposable inbox",
        "usernames" to "Username checker", "compare" to "Compare two passwords", "appearance" to "Appearance",
        "look" to "Style and theme", "themes" to "Theme", "icons" to "Icons", "list" to "Vault list",
        "navigation" to "Navigation", "entry" to "Item page", "codes" to "Codes", "editing" to "Adding and editing",
        "add" to "Add menu", "editor" to "Editor", "generator" to "Generator", "shortcuts" to "Keyboard shortcuts",
        "file" to "Settings file", "about" to "About", "guide" to "Guide",
    )

    /** The hub each page sits under on the phone, where it is not on the settings root. */
    private val PageParents = mapOf(
        "look" to "appearance", "themes" to "look", "icons" to "appearance", "list" to "appearance",
        "navigation" to "appearance", "entry" to "appearance", "codes" to "appearance",
        "add" to "editing", "editor" to "editing", "generator" to "editing",
        "aliases" to "tools", "inbox" to "tools", "usernames" to "tools", "compare" to "tools",
        "vaults" to "sync", "receive" to "sync", "hardware" to "security",
    )
}
