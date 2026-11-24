package io.github.artcraftandroid.procraft.ui

import android.app.Application
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Icon
import androidx.compose.material3.LocalTextStyle
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.scale
import androidx.compose.ui.graphics.drawscope.translate
import androidx.compose.ui.graphics.vector.PathParser
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.artcraftandroid.procraft.AppLog
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import io.github.artcraftandroid.procraft.canvas.KeptBrush
import io.github.artcraftandroid.procraft.canvas.keepBrushesForLater
import io.github.artcraftandroid.procraft.core.Installed
import io.github.artcraftandroid.procraft.core.ItemKind
import io.github.artcraftandroid.procraft.core.SourceInfo
import io.github.artcraftandroid.procraft.core.Store
import io.github.artcraftandroid.procraft.core.StoreItem
import java.io.File
import java.security.MessageDigest
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.util.Locale
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Semaphore
import kotlinx.coroutines.sync.withPermit
import kotlinx.coroutines.withContext

/**
 * The store: the catalogues someone added, what they list, and the one kind of thing this app
 * takes from them. Android has no plugin host and no asset library, so a brush set is installed
 * here (into the library's queue) and the other kinds say plainly where they belong.
 *
 * It is a sheet over the work, as it is on the desk, not a screen of its own: [open] draws it over
 * the gallery or the canvas, which keep running under it, and the reader below is kept here rather
 * than in a screen, so opening it again does not read the store again.
 */
object StoreSheet {
    var open by mutableStateOf(false)
        private set

    /** How many escapes came from the window while the sheet was up, so each one steps back once. */
    internal var escapes by mutableStateOf(0)
        private set

    /**
     * Reads the brush files the store has just kept into the open canvas's library, as the desk's
     * store installs into its own: the app hands this in while a canvas is up. Null while none is,
     * and then the file waits for the next canvas, as it always did.
     */
    internal var takeKeptBrushes: (() -> Unit)? = null

    private var front: StoreFront? = null

    fun show() {
        open = true
    }

    fun hide() {
        open = false
    }

    /**
     * Escape, from the window: the sheet steps back the way its own back key does - what is over
     * the page first, then the page, and from the list it goes.
     */
    internal fun escape() {
        escapes++
    }

    /** The store read from [dir], made the first time the sheet asks for it. */
    internal fun front(dir: File): StoreFront = front ?: StoreFront(dir).also { front = it }
}

/** The sheet over the work: what the app shows, dimmed, with the store's sheet centred on it. */
@Composable
fun StoreOverlay(onTakeKeptBrushes: () -> Unit) {
    // a just-installed set goes into the canvas that is up; once it is gone the file waits as before
    SideEffect { StoreSheet.takeKeptBrushes = onTakeKeptBrushes }
    DisposableEffect(Unit) {
        onDispose { StoreSheet.takeKeptBrushes = null }
    }
    if (StoreSheet.open) {
        val app = LocalContext.current.applicationContext as Application
        val dir = remember(app) { File(app.filesDir, "store") }
        // the app underneath keeps drawing, dimmed; the scrim takes the taps meant for it
        Box(
            Modifier.fillMaxSize().background(SCRIM)
                .clickable(interactionSource = remember { MutableInteractionSource() }, indication = null) { },
            contentAlignment = Alignment.Center,
        ) {
            Box(
                Modifier.fillMaxWidth(SHEET_W).fillMaxHeight(SHEET_H)
                    .clip(RoundedCornerShape(SHEET_CORNER)).background(BG),
            ) {
                StorePage(StoreSheet.front(dir), app)
            }
        }
    }
}

// The sheet, under desktop/src/store.rs's names, so the two can be read side by side.
private val BG = Color(0xFF212121)
private val TITLE = Color(0xFFFFFFFF)
private val ROW_INK = Color(0x99EBEBF5)
private val HINT = Color(0x61EBEBF5)
private val IDLE = Color(0x80EBEBF5)
private val SEARCH_FILL = Color(0x2E767680)
private val PILL_FILL = Color(0x38767680)
private val PILL_STATE_FILL = Color(0x47787880)
private val PILL_ON = Color(0xFFEDEDED)
private val PILL_ON_INK = Color(0xFF1B1B1B)
private val TILE_FILL = Color(0x14FFFFFF)
private val GROUP_FILL = Color(0x0EFFFFFF)
private val SEP = Color(0x99545458)
private val ACCENT get() = Pc.tinted(Color(0xFF0076FD))

// 60 per cent black over the app's screen: what the sheet sits on
private val SCRIM = Color(0x99000000)

// the share of the window it takes, as the desktop's 960 by 780 does of its 1456 by 939
private const val SHEET_W = 0.94f
private const val SHEET_H = 0.80f

private val SHEET_CORNER = 22.dp
private val INSET = 40.dp
private val BAR_HEIGHT = 56.dp
private val TITLE_SIZE = 34.sp
private val TITLE_LINE = 41.dp
private val SEARCH_HEIGHT = 36.dp
private val PILL_HEIGHT = 30.dp
private val PILL_SIDE = 12.dp
private val PILL_GAP = 6.dp
private val FILTERS_W = 76.dp
private val FILTERS_GAP = 8.dp
// the gear: 22 in a 32 hit box, Tabler's own outline settings icon in its 24 unit frame
private val GEAR_GLYPH = 22.dp
private val GEAR_HIT = 32.dp
private const val GEAR_OUTLINE =
    "M10.325 4.317c.426 -1.756 2.924 -1.756 3.35 0a1.724 1.724 0 0 0 2.573 1.066c1.543 -.94 3.31 .826 " +
        "2.37 2.37a1.724 1.724 0 0 0 1.065 2.572c1.756 .426 1.756 2.924 0 3.35a1.724 1.724 0 0 0 -1.066 " +
        "2.573c.94 1.543 -.826 3.31 -2.37 2.37a1.724 1.724 0 0 0 -2.572 1.065c-.426 1.756 -2.924 1.756 " +
        "-3.35 0a1.724 1.724 0 0 0 -2.573 -1.066c-1.543 .94 -3.31 -.826 -2.37 -2.37a1.724 1.724 0 0 0 " +
        "-1.065 -2.572c-1.756 -.426 -1.756 -2.924 0 -3.35a1.724 1.724 0 0 0 1.066 -2.573c-.94 -1.543 " +
        ".826 -3.31 2.37 -2.37c1 .608 2.296 .07 2.572 -1.065"
private const val GEAR_CIRCLE = "M9 12a3 3 0 1 0 6 0a3 3 0 0 0 -6 0"

// the list under the pills, as the desktop lays it out
private val LIST_TOP = 40.dp
private val BAND_HEIGHT = 260.dp
private val BAND_GAP = 24.dp
private val BAND_CORNER = 20.dp
// the band's scrim: nearly clear where the picture starts to show, deep at its foot
private val SCRIM_TOP = Color(0x08000000)
private val SCRIM_FOOT = Color(0xD0000000)
private val HEADING_ABOVE = 40.dp
private val HEADING_BELOW = 16.dp
private val HEADING_LINE = 30.dp
private val SECTION = 22.sp
private val ROW_HEIGHT = 67.dp
private const val ROWS_SHOWN = 4
private val TILE = 42.dp
private val TILE_CORNER = 10.dp
private val TILE_GAP = 12.dp
private val PILL_STATE = 28.dp
private val CARD_CORNER = 16.dp
private const val SHELF_CARDS = 4
private val SHELF_GAP = 13.dp
// the card's foot: the name, then one row holding the second line and the state
private val SHELF_FOOT = 64.dp
// when the state cannot share that row it drops under the line, which takes the card
private val SHELF_FOOT_DROPPED = 92.dp
private val FADE_HEIGHT = 22.dp

// the item page, as item_page() draws it
private val ITEM_ICON = 120.dp
private val ITEM_ICON_CORNER = 27.dp
private val ITEM_NAME = 34.dp
private val CHIP_KIND = 20.dp
private val PILL_BUTTON = 30.dp
private val PILL_BUTTON_SIDE = 18.dp
private val FACTS_HEIGHT = 68.dp
// the item page's picture block, as wide as the page: 377 of 520 tall, the desktop's ratio
private const val ITEM_PICTURE_SHARE = 377f / 520f
private val GROUP_ROW = 56.dp
private val GROUP_CORNER = 10.dp
private val MORE_GAP = 16.dp
private val MORE_FOOT = 56.dp

// the pages behind the bar, as settings_page, source_page, downloads_page, see_all_page draw them
private val GROUP_HEAD = 16.dp
// a grouped list's header rhythm: 16 above the header, 8 between it and what it labels
private val GROUP_HEAD_ABOVE = 16.dp
private val GROUP_HEAD_LINE = 17.dp
private val GROUP_HEAD_BELOW = 8.dp
// the separator's inset from the leading and trailing content edge
private val GROUP_RULE_INSET = 16.dp
private val GROUP_FOOT = 26.dp
private val SWITCH_W = 51.dp
private val SWITCH_H = 31.dp
private val SWITCH_OFF = Color(0x52787880)
private val RECORD_TILE = 34.dp
private val RECORD_TILE_CORNER = 8.dp
private val WALL_GAP = 20.dp
private val WALL_FOOT = 66.dp
private val MENU_W = 262.dp
private val MENU_CORNER = 22.dp
private val MENU_TOP = 50.dp
// the filters menu hangs 6 under the pill: the bar, then the title, the search and the pill row's 30
private val FILTERS_MENU_TOP = BAR_HEIGHT + 121.dp + PILL_HEIGHT + 6.dp
private val MENU_HEAD = 30.dp
private val MENU_ROW = 44.dp
private val ADD_W = 540.dp
private val ADD_H = 300.dp
private val ADD_CORNER = 14.dp
private val ADD_FIELD = 56.dp

// what the pages still borrow from the screen's own palette
private val Dim = Color(0xFF8E8E93)
private val Danger = Color(0xFFE84F51)

/** What a brush set's row says its standing is. */
internal enum class Standing(val text: String) {
    GET("Get"),
    UPDATE("Update"),
    INSTALLED("Installed"),
}

/** What the sheet shows: the list, one item, one source, the settings, the downloads, or a wall. */
private sealed interface SheetPage {
    data object Browse : SheetPage
    data class Item(val id: String) : SheetPage
    data class Source(val id: String) : SheetPage
    data object Settings : SheetPage
    data object Downloads : SheetPage
    data class SeeAll(val kind: ItemKind) : SheetPage
}

/** How the wall orders what it shows, and which way up. */
private enum class SortBy { TITLE, AUTHOR, SIZE }

/**
 * The picture at [address], asked for the first time something wants it: null until it is here,
 * and null for good where the address gave nothing, which is when the glyph stands in.
 */
@Composable
private fun pictureFor(address: String, front: StoreFront): Bitmap? {
    LaunchedEffect(address) { front.want(address) }
    return front.picture(address)
}

/**
 * A catalogue's picture in a card's art rect, cropped to fill it. Painted square: the rounded
 * fill sits under it, so the corners read as the desktop's do, the picture not following them.
 */
@Composable
private fun ArtPicture(bitmap: Bitmap, corner: Dp, modifier: Modifier = Modifier) {
    Box(modifier) {
        Box(Modifier.matchParentSize().clip(RoundedCornerShape(corner)).background(TILE_FILL))
        Image(
            bitmap = bitmap.asImageBitmap(),
            contentDescription = null,
            contentScale = ContentScale.Crop,
            modifier = Modifier.matchParentSize(),
        )
    }
}

/**
 * The store in the app's folder, read off the main thread: its sources, what they list and what
 * was installed, read again after anything changes them.
 */
internal class StoreFront(private val dir: File) {
    var store by mutableStateOf<Store?>(null)
        private set
    var sources by mutableStateOf<List<SourceInfo>>(emptyList())
        private set
    var items by mutableStateOf<List<StoreItem>>(emptyList())
        private set
    var installed by mutableStateOf<Map<String, Installed>>(emptyMap())
        private set
    var failure by mutableStateOf<String?>(null)
        private set

    /** The store opened and read: opening reads every local catalogue again, as the desktop's does. */
    suspend fun open() {
        if (store != null) return
        val opened = withContext(Dispatchers.IO) {
            runCatching { Store.open(dir.path) }.onFailure { AppLog.e("store", "opening the store", it) }.getOrNull()
        }
        if (opened == null) {
            failure = "The store couldn't be opened."
            return
        }
        store = opened
        read()
    }

    suspend fun read() {
        val s = store ?: return
        val read = withContext(Dispatchers.IO) {
            runCatching { Triple(s.sources(), s.items(), s.installed()) }
                .onFailure { AppLog.e("store", "reading the store", it) }
        }
        read.onSuccess { (readSources, readItems, readInstalled) ->
            sources = readSources
            items = readItems
            installed = readInstalled.associateBy { it.id }
        }
        failure = read.exceptionOrNull()?.let { "The store couldn't be read." }
    }

    /** Adds the catalogue at [address] (a web address or a file on the device): null, or why not. */
    suspend fun add(address: String): String? {
        val s = store ?: return "The store isn't open."
        val reason = withContext(Dispatchers.IO) { s.addSource(address) }
        if (reason == null) read()
        return reason
    }

    suspend fun setOn(id: String, on: Boolean) {
        val s = store ?: return
        withContext(Dispatchers.IO) { s.setOn(id, on) }
        read()
    }

    /** Reads [id] again: null when it was read, else why not (the copy it had stays). */
    suspend fun refresh(id: String): String? {
        val s = store ?: return "The store isn't open."
        val reason = withContext(Dispatchers.IO) { s.refresh(id) }
        read()
        return reason
    }

    suspend fun remove(id: String) {
        val s = store ?: return
        withContext(Dispatchers.IO) { s.removeSource(id) }
        read()
    }

    suspend fun search(query: String): List<StoreItem> {
        val s = store ?: return emptyList()
        return withContext(Dispatchers.IO) { s.search(query) }
    }

    /**
     * What [item]'s state is: what the app came with is already in, a record says what was taken,
     * and a record whose digest the catalogue has moved past is an update.
     */
    fun standing(item: StoreItem): Standing {
        val one = installed[item.id]
        val changed = one != null && item.sha256.isNotEmpty() && !one.digest.equals(item.sha256, ignoreCase = true)
        return when {
            changed -> Standing.UPDATE
            one != null || item.file.isEmpty() -> Standing.INSTALLED
            else -> Standing.GET
        }
    }

    /** The categories a catalogue may name, as the pill row lists them. */
    fun categories(): List<String> = store?.categories().orEmpty()

    // the pictures the catalogues gave, by address: fetched once, decoded once, kept with the
    // store; an address that gave nothing is kept as nothing, so it is never asked again
    private val pictures = mutableStateMapOf<String, Bitmap?>()
    private val asking = mutableSetOf<String>()
    private val slots = Semaphore(4)
    private val work = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    /** The picture [address] holds, once it is here; null while it comes, or when it gave nothing. */
    fun picture(address: String): Bitmap? = pictures[address]

    /**
     * Asks for [address] the first time something wants it: a worker of its own, four at a time,
     * the decode off the main thread too. Whatever came back is remembered for good.
     */
    fun want(address: String) {
        if (address.isEmpty() || pictures.containsKey(address) || !asking.add(address)) return
        work.launch {
            val opened = store
            if (opened == null) {
                asking.remove(address)
                return@launch
            }
            val fetched = slots.withPermit { runCatching { opened.fetchPicture(address) }.getOrNull() }
            val picture = fetched?.bytes?.takeIf { it.isNotEmpty() && fetched.failure.isEmpty() }
                ?.let { BitmapFactory.decodeByteArray(it, 0, it.size) }
            withContext(Dispatchers.Main) { pictures[address] = picture }
        }
    }
}

@Composable
private fun StorePage(front: StoreFront, app: Application) {
    val scope = rememberCoroutineScope()
    var page by remember { mutableStateOf<SheetPage>(SheetPage.Browse) }
    // the pages it came through, so back goes up one at a time as the desktop's does
    var history by remember { mutableStateOf(emptyList<SheetPage>()) }
    var query by remember { mutableStateOf("") }
    var category by remember { mutableStateOf(0) }
    var hits by remember { mutableStateOf<List<StoreItem>?>(null) }
    var sort by remember { mutableStateOf(SortBy.TITLE) }
    var ascending by remember { mutableStateOf(true) }
    var sorting by remember { mutableStateOf(false) }
    // the two axes the category row does not carry, and the menu that sets them
    var filtering by remember { mutableStateOf(false) }
    var filterKind by remember { mutableStateOf<ItemKind?>(null) }
    var filterFrom by remember { mutableStateOf<String?>(null) }
    var adding by remember { mutableStateOf(false) }
    // the last try at an item left a problem: its button says so until its reason is asked for
    var problem by remember { mutableStateOf<Pair<String, String>?>(null) }
    // what the sheet has to say itself: a reason, or the question before a source goes
    var said by remember { mutableStateOf<Pair<String, String>?>(null) }
    var removing by remember { mutableStateOf<SourceInfo?>(null) }

    LaunchedEffect(front) { front.open() }
    LaunchedEffect(query, front.items) {
        hits = if (query.isBlank()) null else front.search(query)
    }

    fun go(next: SheetPage) {
        history = history + page
        page = next
    }

    fun stepBack() {
        val previous = history.lastOrNull()
        if (previous != null) {
            history = history.dropLast(1)
            page = previous
            return
        }
        if (page !is SheetPage.Browse) page = SheetPage.Browse else StoreSheet.hide()
    }

    // what the page really is: an item or a source that has gone falls back to the list under it
    val item = (page as? SheetPage.Item)?.let { at -> front.items.firstOrNull { it.id == at.id } }
    val source = (page as? SheetPage.Source)?.let { at -> front.sources.firstOrNull { it.id == at.id } }
    val showing: SheetPage = when (page) {
        is SheetPage.Item -> if (item != null) page else SheetPage.Browse
        is SheetPage.Source -> if (source != null) page else SheetPage.Browse
        else -> page
    }

    // back takes what is over the page first, then goes up one page, and from the list it leaves
    fun stepOut() {
        when {
            said != null -> said = null
            removing != null -> removing = null
            adding -> adding = false
            filtering -> filtering = false
            sorting -> sorting = false
            else -> stepBack()
        }
    }
    BackHandler(onBack = ::stepOut)
    // and the window's escape does the same, once for each one it sends, as ui::escape does
    var told by remember { mutableStateOf(StoreSheet.escapes) }
    LaunchedEffect(StoreSheet.escapes) {
        if (StoreSheet.escapes != told) {
            told = StoreSheet.escapes
            stepOut()
        }
    }

    Box(Modifier.fillMaxSize()) {
        Column(Modifier.fillMaxSize()) {
            // the bar stays put while the page under it scrolls: from the list it puts the sheet
            // away, from a page it goes back, with the page's name at its middle
            when (showing) {
                is SheetPage.Browse -> StoreBar(
                    onBack = ::stepBack,
                    onSettings = { go(SheetPage.Settings) },
                    onDownloads = { go(SheetPage.Downloads) },
                )
                is SheetPage.Item -> StoreBar(::stepBack, item?.name, onDownloads = { go(SheetPage.Downloads) })
                is SheetPage.Source -> StoreBar(::stepBack, source?.name, onDownloads = { go(SheetPage.Downloads) })
                is SheetPage.Settings -> StoreBar(::stepBack, "Settings", onDownloads = { go(SheetPage.Downloads) })
                // nothing downloads from the page about downloads
                is SheetPage.Downloads -> StoreBar(::stepBack, "Downloads")
                is SheetPage.SeeAll -> StoreBar(
                    ::stepBack, sectionTitle(showing.kind),
                    onSort = { sorting = true }, onDownloads = { go(SheetPage.Downloads) },
                )
            }
            when (showing) {
                // the list keeps its own header and scrolls only what is under the pills
                is SheetPage.Browse -> BrowsePage(
                    front, query, { query = it }, hits, category, { category = it },
                    kind = filterKind, from = filterFrom,
                    onFilters = { filtering = !filtering },
                    onSeeAll = { go(SheetPage.SeeAll(it)) },
                ) { go(SheetPage.Item(it.id)) }
                // the pages under the bar: 40 in from the sides, 20 above the sheet's foot
                is SheetPage.Item -> PageScroll {
                    val open = item
                    if (open != null) ItemPage(
                        front, open, app,
                        problem = problem?.takeIf { it.first == open.id }?.second,
                        onProblem = { why -> problem = why?.let { open.id to it } },
                        onTell = { said = "Not installed" to it },
                        onItem = { other -> go(SheetPage.Item(other.id)) },
                    )
                }
                is SheetPage.Settings -> PageScroll {
                    SettingsPage(
                        front,
                        onSource = { go(SheetPage.Source(it.id)) },
                        onAdd = { adding = true },
                    )
                }
                is SheetPage.Source -> PageScroll {
                    val shown = source
                    if (shown != null) SourcePage(
                        front, shown,
                        onRefresh = {
                            scope.launch {
                                val why = front.refresh(shown.id)
                                if (why != null) said = "Not read" to why
                            }
                        },
                        onRemove = { removing = shown },
                    )
                }
                is SheetPage.Downloads -> PageScroll {
                    DownloadsPage(front) { go(SheetPage.Item(it.id)) }
                }
                is SheetPage.SeeAll -> PageScroll {
                    SeeAllPage(front, showing.kind, sort, ascending) { go(SheetPage.Item(it.id)) }
                }
            }
        }
        // the menu the wall's bar opens: the order, one radio a line
        if (sorting) {
            SortMenu(
                sort, ascending,
                onSort = { sort = it; sorting = false },
                onAscending = { ascending = it; sorting = false },
                onClose = { sorting = false },
            )
        }
        // the menu the category row's Filters pill opens: the kind, then the source, one radio a line
        if (filtering) {
            FiltersMenu(
                front, filterKind, filterFrom,
                onKind = { filterKind = it; filtering = false },
                onFrom = { filterFrom = it; filtering = false },
                onClose = { filtering = false },
            )
        }
        // adding a source: its own panel over the sheet, never a window of the system's
        if (adding) {
            AddSheet(front) { adding = false }
        }
        // the sheet's own alerts, over it: what it has to say, and the question before a removal
        Alert(
            value = said,
            title = { it.first },
            message = { it.second },
            action = "OK",
            onCancel = { said = null },
            onAction = { said = null },
            alone = true,
            destructive = false,
        )
        Alert(
            value = removing,
            title = { "Remove this source?" },
            message = {
                "\"${it.name}\" is forgotten. Its items leave the store, and things installed from it stay installed."
            },
            action = "Remove",
            onCancel = { removing = null },
            onAction = { gone ->
                removing = null
                scope.launch {
                    front.remove(gone.id)
                    stepBack()
                }
            },
        )
    }
}

/** The pages under the bar: 40 in from the sides and 20 above the sheet's foot, scrolled. */
@Composable
private fun ColumnScope.PageScroll(content: @Composable ColumnScope.() -> Unit) {
    Column(
        Modifier.weight(1f).verticalScroll(rememberScrollState())
            .padding(start = INSET, end = INSET, bottom = 20.dp),
        content = content,
    )
}

@Composable
private fun BrowsePage(
    front: StoreFront,
    query: String,
    onQuery: (String) -> Unit,
    hits: List<StoreItem>?,
    category: Int,
    onCategory: (Int) -> Unit,
    kind: ItemKind?,
    from: String?,
    onFilters: () -> Unit,
    onSeeAll: (ItemKind) -> Unit,
    onItem: (StoreItem) -> Unit,
) {
    Column(Modifier.fillMaxSize().padding(horizontal = INSET)) {
        // under the bar: the title on its own line, then the search field, then the categories
        Box(
            Modifier.fillMaxWidth().padding(top = 12.dp).height(TITLE_LINE),
            contentAlignment = Alignment.CenterStart,
        ) {
            Text("Store", color = TITLE, fontSize = TITLE_SIZE, fontWeight = FontWeight.Bold)
        }
        Spacer(Modifier.height(16.dp))
        SearchField(query, onQuery)
        Spacer(Modifier.height(16.dp))
        CategoryRow(listOf("All") + front.categories(), category, onCategory, filtered = kind != null || from != null, onFilters = onFilters)
        // a source that could not be read says so once above the list, in the room the desktop
        // keeps for it, with the day of the copy it still shows
        val stale = front.sources.firstOrNull { it.failure.isNotEmpty() && it.read != 0uL }
        Box(Modifier.fillMaxWidth().height(12.dp + if (stale != null) 26.dp else 0.dp)) {
            if (stale != null) {
                Row(
                    Modifier.offset(y = 12.dp).fillMaxWidth().height(26.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    InfoGlyph(size = 13.dp, stroke = 1.2.dp, stem = 3f to 3.5f, dot = 0.8.dp, dotAt = 5.dp)
                    Spacer(Modifier.width(7.dp))
                    Text(
                        "${stale.name} could not be refreshed. Showing what it had on ${readText(stale.read)}.",
                        color = ROW_INK, fontSize = 13.sp, maxLines = 1, overflow = TextOverflow.Ellipsis,
                    )
                }
            }
        }

        // the categories, the search and the filters cut the list, as the desktop's page does
        val chosen = front.categories().getOrNull(category - 1)
        val items = (hits ?: front.items)
            .filter { chosen == null || it.category == chosen }
            .filter { kind == null || it.kind == kind }
            .filter { from == null || it.source == from }
        val failure = front.failure
        val scroll = rememberScrollState()
        Box(Modifier.weight(1f).fillMaxWidth()) {
            when {
                front.store == null -> Text(
                    "Reading the store...",
                    color = Dim, fontSize = 13.sp, modifier = Modifier.padding(top = 24.dp),
                )
                failure != null -> Text(failure, color = Danger, fontSize = 13.sp, modifier = Modifier.padding(top = 8.dp))
                items.isEmpty() -> Text(
                    when {
                        query.isNotBlank() -> "No results for ${query.trim()}."
                        chosen != null -> "Nothing here in this category."
                        else -> "Every source is off. Turn one on in Sources."
                    },
                    color = ROW_INK, fontSize = 15.sp, fontWeight = FontWeight.Medium,
                    textAlign = TextAlign.Center, modifier = Modifier.fillMaxWidth().padding(top = 48.dp),
                )
                else -> {
                    Column(Modifier.fillMaxSize().verticalScroll(scroll)) {
                        StoreList(
                            items, front,
                            banded = query.isBlank() && chosen == null && kind == null && from == null,
                            onItem = onItem, onSeeAll = onSeeAll,
                        )
                    }
                    // the list's top edge dissolves under the pills once it has moved
                    if (scroll.value > 0) {
                        Box(
                            Modifier.fillMaxWidth().height(FADE_HEIGHT)
                                .background(Brush.verticalGradient(listOf(BG, Color.Transparent))),
                        )
                    }
                }
            }
        }
    }
}

/** The desktop's list: the band's place at the top, then a section for each kind, then what is in. */
@Composable
private fun StoreList(
    items: List<StoreItem>,
    front: StoreFront,
    banded: Boolean,
    onItem: (StoreItem) -> Unit,
    onSeeAll: (ItemKind) -> Unit,
) {
    val updates = items.filter { front.standing(it) == Standing.UPDATE }
    val brushes = items.filter { it.kind == ItemKind.BRUSH_SET }
    val plugins = items.filter { it.kind == ItemKind.PLUGIN }
    val packs = items.filter { it.kind == ItemKind.ASSET_PACK }
    // what the records hold, in the order they were taken, as the desktop puts it at the foot
    val took = front.installed.values.mapNotNull { one -> items.firstOrNull { it.id == one.id } }

    Column(Modifier.fillMaxWidth()) {
        Spacer(Modifier.height(LIST_TOP))
        if (banded) {
            // the first thing listed that has a picture, or the first there is, drawn large over
            // the list as the page opens
            val featured = items.firstOrNull { it.preview.isNotEmpty() } ?: items.first()
            Band(featured, front, onItem)
            Spacer(Modifier.height(BAND_GAP))
        }
        if (updates.isNotEmpty()) {
            StoreSection("Updates", more = false) {
                updates.take(ROWS_SHOWN).forEachIndexed { at, item -> ItemRow(item, front, ruled = at > 0, onItem) }
            }
        }
        if (brushes.isNotEmpty()) {
            StoreSection("Brushes", more = brushes.size > ROWS_SHOWN, onSeeAll = { onSeeAll(ItemKind.BRUSH_SET) }) {
                Shelf(brushes, front, onItem)
            }
        }
        if (plugins.isNotEmpty()) {
            StoreSection("Plugins", more = plugins.size > ROWS_SHOWN, onSeeAll = { onSeeAll(ItemKind.PLUGIN) }) {
                plugins.take(ROWS_SHOWN).forEachIndexed { at, item -> ItemRow(item, front, ruled = at > 0, onItem) }
            }
        }
        if (packs.isNotEmpty()) {
            StoreSection("Asset packs", more = packs.size > ROWS_SHOWN, onSeeAll = { onSeeAll(ItemKind.ASSET_PACK) }) {
                Shelf(packs, front, onItem)
            }
        }
        if (took.isNotEmpty()) {
            StoreSection("Installed", more = false) { Shelf(took, front, onItem) }
        }
    }
}

/**
 * The band: the first thing the store lists, drawn large over the list and opening it when
 * tapped. The picture the catalogue gave would fill it; the kind's own glyph stands there until
 * the pictures come.
 */
@Composable
private fun Band(item: StoreItem, front: StoreFront, onItem: (StoreItem) -> Unit) {
    val shape = RoundedCornerShape(BAND_CORNER)
    // the words sit on the band's own 20 in from its side, and 20 from its other end
    val words = Modifier.offset(x = 20.dp).fillMaxWidth().padding(end = 40.dp)
    val picture = pictureFor(item.preview, front)
    BoxWithConstraints(
        Modifier.fillMaxWidth().height(BAND_HEIGHT).clip(shape).background(TILE_FILL)
            .border(1.dp, SEP, shape).clickable { onItem(item) },
    ) {
        val bandWidth = maxWidth
        if (picture != null) {
            // the catalogue's picture fills it, cropped, rounded with the band's own mesh as the
            // desktop rounds it, under a scrim that deepens toward the foot
            Image(
                bitmap = picture.asImageBitmap(),
                contentDescription = null,
                contentScale = ContentScale.Crop,
                modifier = Modifier.matchParentSize(),
            )
            Box(
                Modifier.align(Alignment.BottomStart).fillMaxWidth().height(BAND_HEIGHT * 0.8f)
                    .clip(shape)
                    .background(Brush.verticalGradient(listOf(SCRIM_TOP, SCRIM_FOOT))),
            )
        } else {
            // the kind's glyph, large and quiet, at the top right
            Icon(
                kindIcon(item.kind),
                contentDescription = null,
                tint = IDLE.copy(alpha = IDLE.alpha * 0.35f),
                modifier = Modifier.align(Alignment.TopEnd).padding(top = 24.dp, end = 50.dp).size(120.dp),
            )
        }
        // the words stack above the foot: the kicker, the name, one sentence, then the app row
        Text(
            item.category.uppercase(),
            color = ROW_INK, fontSize = 12.sp, fontWeight = FontWeight.SemiBold,
            modifier = Modifier.offset(x = 20.dp, y = 122.dp),
        )
        Text(
            item.name,
            color = TITLE, fontSize = 30.sp, fontWeight = FontWeight.Bold,
            maxLines = 1, overflow = TextOverflow.Ellipsis,
            modifier = words.offset(y = 144.dp),
        )
        if (item.description.isNotEmpty()) {
            // as much of the sentence as fits one line, ended with " ..." where it was cut, the
            // way one_line() builds it: the same words, the same place, the same dots
            Text(
                oneLine(item.description, bandWidth - 40.dp),
                color = ROW_INK, fontSize = 15.sp, maxLines = 1,
                modifier = words.offset(y = 182.dp),
            )
        }
        // its app row: the kind's tile, whose it is, and the state at the end
        Box(Modifier.offset(y = 218.dp).then(words).height(PILL_STATE)) {
            Box(
                Modifier.align(Alignment.CenterStart).size(24.dp)
                    .clip(RoundedCornerShape(6.dp)).background(PILL_FILL),
                contentAlignment = Alignment.Center,
            ) {
                Icon(kindIcon(item.kind), contentDescription = null, tint = TITLE, modifier = Modifier.size(14.dp))
            }
            Text(
                item.publisher,
                color = ROW_INK, fontSize = 13.sp, maxLines = 1, overflow = TextOverflow.Ellipsis,
                modifier = Modifier.align(Alignment.CenterStart).padding(start = 32.dp),
            )
            StatePill(front.standing(item), Modifier.align(Alignment.CenterEnd))
        }
    }
}

/** As much of [sentence] as fits [width] on one line, ended with " ..." where it was cut. */
@Composable
private fun oneLine(sentence: String, width: Dp): String {
    val measurer = rememberTextMeasurer()
    // the theme's own face and tracking, so what is measured is what is drawn
    val style = LocalTextStyle.current.merge(TextStyle(fontSize = 15.sp))
    val limit = with(LocalDensity.current) { width.toPx() }
    var kept = ""
    for (word in sentence.split(' ')) {
        val trying = if (kept.isEmpty()) word else "$kept $word"
        if (measurer.measure("$trying ...", style).size.width > limit) {
            return if (kept.isEmpty()) "" else "$kept ..."
        }
        kept = trying
    }
    return kept
}

/**
 * The small i the store's prose carries: a ring, a stem and a dot, as the desktop draws it, at
 * the item page's 14 dp and at the stale line's 13.
 */
@Composable
private fun InfoGlyph(
    modifier: Modifier = Modifier,
    size: Dp = 14.dp,
    stroke: Dp = 1.4.dp,
    stem: Pair<Float, Float> = 3f to 4f,
    dot: Dp = 0.9.dp,
    dotAt: Dp = 5.5.dp,
) {
    Canvas(modifier.size(size)) {
        val half = this.size.minDimension / 2
        drawCircle(IDLE, radius = half, style = Stroke(stroke.toPx()))
        drawLine(
            IDLE,
            Offset(center.x, center.y - stem.first.dp.toPx()),
            Offset(center.x, center.y + stem.second.dp.toPx()),
            stroke.toPx(),
            StrokeCap.Round,
        )
        drawCircle(IDLE, radius = dot.toPx(), center = Offset(center.x, center.y - dotAt.toPx()))
    }
}

/** A section's heading, 40 under the one before it, then what it lists 16 under that. */
@Composable
private fun ColumnScope.StoreSection(
    heading: String,
    more: Boolean,
    onSeeAll: () -> Unit = {},
    rows: @Composable ColumnScope.() -> Unit,
) {
    Spacer(Modifier.height(HEADING_ABOVE))
    Box(Modifier.fillMaxWidth().height(HEADING_LINE)) {
        Text(heading, color = TITLE, fontSize = SECTION, fontWeight = FontWeight.Bold, modifier = Modifier.align(Alignment.TopStart))
        if (more) {
            // the kind's own wall, as the desktop opens it from here
            Row(Modifier.align(Alignment.CenterEnd).clickable { onSeeAll() }, verticalAlignment = Alignment.CenterVertically) {
                Text("See all", color = ACCENT, fontSize = 15.sp)
                Spacer(Modifier.width(3.dp))
                Icon(PcIcons.ChevronRight, contentDescription = null, tint = ACCENT, modifier = Modifier.size(11.dp))
            }
        }
    }
    Spacer(Modifier.height(HEADING_BELOW))
    rows()
}

/** A shelf of cards, four across: the art, the name, one line under it and the state. */
@Composable
private fun Shelf(items: List<StoreItem>, front: StoreFront, onItem: (StoreItem) -> Unit) {
    val measurer = rememberTextMeasurer()
    val density = LocalDensity.current
    val base = LocalTextStyle.current
    BoxWithConstraints(Modifier.fillMaxWidth()) {
        // a card takes what is left of the row once its three gaps are out
        val each = (maxWidth - SHELF_GAP * (SHELF_CARDS - 1)) / SHELF_CARDS
        Row(horizontalArrangement = Arrangement.spacedBy(SHELF_GAP)) {
            for (item in items.take(SHELF_CARDS)) {
                val picture = pictureFor(item.preview, front)
                val standing = front.standing(item)
                // the row is shared while the line and the state both fit on the card, as the
                // wall and More-from cards do; otherwise the state drops under the line
                val sharesLine = with(density) {
                    val line = measurer.measure(secondLine(item), base.merge(TextStyle(fontSize = 13.sp))).size.width.toDp()
                    val pill = measurer.measure(
                        standing.text,
                        base.merge(TextStyle(fontSize = 15.sp, fontWeight = FontWeight.SemiBold)),
                    ).size.width.toDp() + 28.dp + (if (standing == Standing.INSTALLED) 21.dp else 0.dp)
                    line + 8.dp + pill <= each
                }
                Column(Modifier.width(each).clickable { onItem(item) }) {
                    if (picture != null) {
                        ArtPicture(picture, CARD_CORNER, Modifier.fillMaxWidth().height(each * 0.75f))
                    } else {
                        Box(
                            Modifier.fillMaxWidth().height(each * 0.75f)
                                .clip(RoundedCornerShape(CARD_CORNER)).background(TILE_FILL),
                            contentAlignment = Alignment.Center,
                        ) {
                            Icon(kindIcon(item.kind), contentDescription = null, tint = IDLE, modifier = Modifier.size(36.dp))
                        }
                    }
                    Box(Modifier.fillMaxWidth().height(if (sharesLine) SHELF_FOOT else SHELF_FOOT_DROPPED)) {
                        Text(
                            item.name,
                            color = TITLE, fontSize = 15.sp, fontWeight = FontWeight.SemiBold,
                            maxLines = 1, overflow = TextOverflow.Ellipsis,
                            modifier = Modifier.offset(y = 10.dp).fillMaxWidth(),
                        )
                        if (sharesLine) {
                            // the line and the state on one row, the state's right edge on the card's
                            Row(
                                Modifier.offset(y = 26.dp).fillMaxWidth().height(PILL_STATE),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                Text(
                                    secondLine(item),
                                    color = ROW_INK, fontSize = 13.sp, maxLines = 1, overflow = TextOverflow.Ellipsis,
                                    modifier = Modifier.weight(1f),
                                )
                                Spacer(Modifier.width(8.dp))
                                StatePill(front.standing(item))
                            }
                        } else {
                            // no room for both on one row: the line takes the card and the state drops
                            // under it, still on the card's right
                            Text(
                                secondLine(item),
                                color = ROW_INK, fontSize = 13.sp, maxLines = 1, overflow = TextOverflow.Ellipsis,
                                modifier = Modifier.offset(y = 32.dp).fillMaxWidth(),
                            )
                            StatePill(front.standing(item), Modifier.align(Alignment.TopEnd).offset(y = 60.dp))
                        }
                    }
                }
            }
        }
    }
}

/** One row: its kind's tile, the name, one line under it, and the state at the trailing end. */
@Composable
private fun ItemRow(item: StoreItem, front: StoreFront, ruled: Boolean, onItem: (StoreItem) -> Unit) {
    val picture = pictureFor(item.preview, front)
    Box(Modifier.fillMaxWidth().height(ROW_HEIGHT).clickable { onItem(item) }) {
        // every row but a group's first carries the rule above it
        if (ruled) Box(Modifier.fillMaxWidth().height(1.dp).background(SEP))
        Row(Modifier.fillMaxSize(), verticalAlignment = Alignment.CenterVertically) {
            if (picture != null) {
                ArtPicture(picture, TILE_CORNER, Modifier.size(TILE))
            } else {
                Box(
                    Modifier.size(TILE).clip(RoundedCornerShape(TILE_CORNER)).background(TILE_FILL),
                    contentAlignment = Alignment.Center,
                ) {
                    Icon(kindIcon(item.kind), contentDescription = null, tint = IDLE, modifier = Modifier.size(24.dp))
                }
            }
            Spacer(Modifier.width(TILE_GAP))
            Column(Modifier.weight(1f)) {
                Text(
                    item.name,
                    color = TITLE, fontSize = 17.sp, lineHeight = 22.sp,
                    maxLines = 1, overflow = TextOverflow.Ellipsis,
                )
                Text(
                    secondLine(item),
                    color = ROW_INK, fontSize = 15.sp, lineHeight = 22.sp,
                    maxLines = 1, overflow = TextOverflow.Ellipsis,
                )
            }
            StatePill(front.standing(item))
        }
    }
}

/** The state a card or a row ends with: Get, Update or Installed, with the tick once it is in. */
@Composable
private fun StatePill(standing: Standing, modifier: Modifier = Modifier) {
    val update = standing == Standing.UPDATE
    val tick = standing == Standing.INSTALLED
    Row(
        modifier.height(PILL_STATE).clip(RoundedCornerShape(PILL_STATE / 2))
            .background(if (update) ACCENT else PILL_STATE_FILL).padding(horizontal = 14.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (tick) {
            Icon(PcIcons.Tick, contentDescription = null, tint = ROW_INK, modifier = Modifier.size(15.dp))
            Spacer(Modifier.width(6.dp))
        }
        Text(
            standing.text,
            color = if (update) TITLE else if (tick) ROW_INK else ACCENT,
            fontSize = 15.sp,
            fontWeight = FontWeight.SemiBold,
        )
    }
}

/** The top of the sheet, 56 tall: the way back, the page's name, and what the page offers. */
@Composable
private fun StoreBar(
    onBack: () -> Unit,
    name: String? = null,
    onSettings: (() -> Unit)? = null,
    onSort: (() -> Unit)? = null,
    onDownloads: (() -> Unit)? = null,
) {
    Row(Modifier.fillMaxWidth().padding(horizontal = INSET).height(BAR_HEIGHT), verticalAlignment = Alignment.CenterVertically) {
        Icon(
            PcIcons.ChevronLeft,
            contentDescription = "Back",
            tint = TITLE,
            modifier = Modifier.size(22.dp).clickable(onClick = onBack),
        )
        if (name != null) {
            // on the sheet's own middle: the chevron and what follows it take as much at either end
            Text(
                name,
                color = TITLE,
                fontSize = 17.sp,
                fontWeight = FontWeight.SemiBold,
                textAlign = TextAlign.Center,
                maxLines = 1,
                modifier = Modifier.weight(1f).padding(horizontal = 12.dp),
            )
        } else {
            Spacer(Modifier.weight(1f))
        }
        if (onSort != null) {
            SortGlyph(Modifier.clickable { onSort() })
            Spacer(Modifier.width(23.dp))
        }
        if (onSettings != null) {
            // the gear hard on the sheet's inset, the downloads one step in from it, as the
            // desktop orders the two at the bar's trailing end
            DownloadsGlyph(Modifier.clickable { onDownloads?.invoke() })
            Spacer(Modifier.width(8.dp))
            GearGlyph(Modifier.clickable { onSettings() }.offset(x = 5.dp))
        } else if (onDownloads != null) {
            DownloadsGlyph(Modifier.clickable { onDownloads() })
        } else {
            // the downloads page has no arrow to itself, and the name keeps its middle
            Spacer(Modifier.width(22.dp))
        }
    }
}

/**
 * Tabler's settings gear, 22 in a 32 hit box: one outline of eight rounded teeth round a circle,
 * strokes 2 in its 24 unit frame with round caps and joins.
 */
@Composable
private fun GearGlyph(modifier: Modifier = Modifier) {
    val outline = remember { PathParser().parsePathString(GEAR_OUTLINE).toPath() }
    val circle = remember { PathParser().parsePathString(GEAR_CIRCLE).toPath() }
    Canvas(modifier.size(GEAR_HIT)) {
        // the 22 dp glyph inside the box, its 24 unit frame scaled to that
        val glyph = size.minDimension * (GEAR_GLYPH / GEAR_HIT)
        val frame = glyph / 24f
        translate((size.width - glyph) / 2f, (size.height - glyph) / 2f) {
            scale(frame, pivot = Offset.Zero) {
                val stroke = Stroke(2f, cap = StrokeCap.Round, join = StrokeJoin.Round)
                drawPath(outline, TITLE, style = stroke)
                drawPath(circle, TITLE, style = stroke)
            }
        }
    }
}

/** The two arrows the sort button carries: one up, one down. */
@Composable
private fun SortGlyph(modifier: Modifier = Modifier) {
    Canvas(modifier.size(22.dp)) {
        val d = size.minDimension / 2
        val stroke = 1.8.dp.toPx()
        val left = center.x - d * 0.45f
        val right = center.x + d * 0.45f
        drawLine(TITLE, Offset(left, center.y + d * 0.55f), Offset(left, center.y - d * 0.55f), stroke, StrokeCap.Round)
        drawLine(TITLE, Offset(left - d * 0.32f, center.y - d * 0.22f), Offset(left, center.y - d * 0.55f), stroke, StrokeCap.Round)
        drawLine(TITLE, Offset(right, center.y - d * 0.55f), Offset(right, center.y + d * 0.55f), stroke, StrokeCap.Round)
        drawLine(TITLE, Offset(right + d * 0.32f, center.y + d * 0.22f), Offset(right, center.y + d * 0.55f), stroke, StrokeCap.Round)
    }
}

/** The downloads arrow and its tray: the same four lines the desktop draws. */
@Composable
private fun DownloadsGlyph(modifier: Modifier = Modifier) {
    Canvas(modifier.size(22.dp)) {
        val d = size.minDimension / 2
        val stroke = 1.8.dp.toPx()
        val tip = Offset(center.x, center.y + d * 0.1f)
        drawLine(TITLE, Offset(center.x, center.y - d * 0.8f), tip, stroke, StrokeCap.Round)
        drawLine(TITLE, Offset(center.x - d * 0.42f, center.y - d * 0.3f), tip, stroke, StrokeCap.Round)
        drawLine(TITLE, Offset(center.x + d * 0.42f, center.y - d * 0.3f), tip, stroke, StrokeCap.Round)
        drawLine(TITLE, Offset(center.x - d * 0.8f, center.y + d * 0.7f), Offset(center.x + d * 0.8f, center.y + d * 0.7f), stroke, StrokeCap.Round)
    }
}

/** The search field: 36 tall, the magnifier 14 in from its edge, the word Search while it is empty. */
@Composable
private fun SearchField(value: String, onValue: (String) -> Unit) {
    Box(Modifier.fillMaxWidth().height(SEARCH_HEIGHT).clip(RoundedCornerShape(10.dp)).background(SEARCH_FILL)) {
        MagnifierGlyph(Modifier.align(Alignment.CenterStart).padding(start = 7.dp))
        BasicTextField(
            value = value,
            onValueChange = onValue,
            singleLine = true,
            textStyle = TextStyle(color = TITLE, fontSize = 17.sp),
            cursorBrush = SolidColor(ACCENT),
            modifier = Modifier.align(Alignment.CenterStart).fillMaxWidth().padding(start = 30.dp, end = 10.dp),
            decorationBox = { field ->
                Box {
                    if (value.isEmpty()) Text("Search", color = HINT, fontSize = 17.sp)
                    field()
                }
            },
        )
    }
}

/** The magnifier the search field carries: a ring with its handle. */
@Composable
private fun MagnifierGlyph(modifier: Modifier = Modifier) {
    Canvas(modifier.size(16.dp)) {
        val stroke = 1.6.dp.toPx()
        drawCircle(HINT, radius = 5.dp.toPx(), center = Offset(7.dp.toPx(), 7.dp.toPx()), style = Stroke(stroke))
        drawLine(HINT, Offset(10.6.dp.toPx(), 10.6.dp.toPx()), Offset(14.dp.toPx(), 14.dp.toPx()), stroke, StrokeCap.Round)
    }
}

/**
 * The categories as pills 6 apart, with the Filters pill pinned at the trailing edge. The desktop's
 * rule: a pill that would cross the Filters pill's 8 dp is not drawn, and the wider sheet is what
 * keeps that from happening at this width.
 */
@Composable
private fun CategoryRow(
    categories: List<String>,
    chosen: Int,
    onChosen: (Int) -> Unit,
    filtered: Boolean,
    onFilters: () -> Unit,
) {
    val measurer = rememberTextMeasurer()
    // the theme's own face and tracking, so what is measured is what is drawn
    val pillText = LocalTextStyle.current.merge(TextStyle(fontSize = 13.sp))
    BoxWithConstraints(Modifier.fillMaxWidth().height(PILL_HEIGHT)) {
        val room = with(LocalDensity.current) { (maxWidth - FILTERS_GAP - FILTERS_W).toPx() }
        val shown = with(LocalDensity.current) {
            var at = 0
            var taken = 0f
            for (name in categories) {
                val pill = measurer.measure(name, pillText).size.width + 2 * PILL_SIDE.toPx()
                val need = if (at == 0) pill else pill + PILL_GAP.toPx()
                if (taken + need > room) break
                taken += need
                at++
            }
            at
        }
        Row(Modifier.fillMaxWidth().height(PILL_HEIGHT), verticalAlignment = Alignment.CenterVertically) {
            Row(
                horizontalArrangement = Arrangement.spacedBy(PILL_GAP),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                categories.take(shown).forEachIndexed { at, name -> CategoryPill(name, at == chosen) { onChosen(at) } }
            }
            Spacer(Modifier.weight(1f))
            Box(
                Modifier.width(FILTERS_W).height(PILL_HEIGHT).clip(RoundedCornerShape(PILL_HEIGHT / 2))
                    .background(PILL_FILL).clickable(onClick = onFilters),
                contentAlignment = Alignment.Center,
            ) {
                Text("Filters", color = if (filtered) ACCENT else TITLE, fontSize = 13.sp)
            }
        }
    }
}

/** One category: the pill's own fill, or the chosen one's. */
@Composable
private fun CategoryPill(name: String, on: Boolean, onClick: () -> Unit) {
    Box(
        Modifier.height(PILL_HEIGHT).clip(RoundedCornerShape(PILL_HEIGHT / 2)).background(if (on) PILL_ON else PILL_FILL)
            .clickable(onClick = onClick).padding(horizontal = PILL_SIDE),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            name,
            color = if (on) PILL_ON_INK else TITLE,
            fontSize = 13.sp,
            fontWeight = if (on) FontWeight.SemiBold else FontWeight.Normal,
        )
    }
}

/**
 * One item's own page: the head with its one action, what taking it means, its facts, its words,
 * its details, and the maker's other work.
 */
@Composable
private fun ItemPage(
    front: StoreFront,
    item: StoreItem,
    app: Application,
    problem: String?,
    onProblem: (String?) -> Unit,
    onTell: (String) -> Unit,
    onItem: (StoreItem) -> Unit,
) {
    val scope = rememberCoroutineScope()
    var busy by remember(item.id) { mutableStateOf(false) }
    val standing = front.standing(item)
    val isIn = standing != Standing.GET
    val update = standing == Standing.UPDATE
    // the button's word, in the desktop's order: on its way, then a problem, then an update
    val label = when {
        busy -> "Installing"
        problem != null -> "Not installed"
        update -> "Update"
        isIn -> "Installed"
        else -> "Install"
    }
    // the accent while there is something to do, and the neutral fill when there is not
    val filled = problem == null && (!isIn || update || busy)
    val acts = !busy && (problem != null || !isIn || update)
    // its width is its word's, 18 each side, as the desktop measures it
    val measurer = rememberTextMeasurer()
    val pillWidth = with(LocalDensity.current) {
        measurer.measure(label, TextStyle(fontSize = 15.sp, fontWeight = FontWeight.SemiBold)).size.width.toDp() +
            PILL_BUTTON_SIDE * 2
    }
    // the name and the chip run from past the icon, under the button where they are long, as the
    // desktop lets them: it draws the button over whatever reaches it
    val beside = Modifier.padding(start = ITEM_ICON + 20.dp).fillMaxWidth()

    fun act() {
        if (!acts) return
        if (problem != null) {
            // the reason only when it is asked for, in the store's own alert
            onProblem(null)
            onTell(problem)
            return
        }
        busy = true
        scope.launch {
            val why = installBrushSet(front.store, item, app)
            busy = false
            if (why == null) front.read() else onProblem(why)
        }
    }

    // the head: the icon, the name with its kind, the publisher, and the one action
    Spacer(Modifier.height(24.dp))
    Box(Modifier.fillMaxWidth().height(ITEM_ICON)) {
        Box(
            Modifier.size(ITEM_ICON).clip(RoundedCornerShape(ITEM_ICON_CORNER)).background(TILE_FILL),
            contentAlignment = Alignment.Center,
        ) {
            Icon(kindIcon(item.kind), contentDescription = null, tint = IDLE, modifier = Modifier.size(56.dp))
        }
        // the name and the kind's chip, on the icon's 34th dp
        Row(
            beside.offset(y = ITEM_NAME - 21.dp).height(42.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                item.name,
                color = TITLE, fontSize = 30.sp, fontWeight = FontWeight.Bold,
                maxLines = 1, overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f, fill = false),
            )
            Spacer(Modifier.width(10.dp))
            Box(
                Modifier.height(CHIP_KIND).clip(RoundedCornerShape(6.dp)).background(TILE_FILL).padding(horizontal = 7.dp),
                contentAlignment = Alignment.Center,
            ) {
                Text(kindText(item.kind).uppercase(), color = ROW_INK, fontSize = 11.sp, fontWeight = FontWeight.SemiBold)
            }
        }
        // whose it is, in the accent, under the name
        Box(beside.offset(y = ITEM_NAME + 16.dp).height(28.dp), contentAlignment = Alignment.CenterStart) {
            Text(item.publisher, color = ACCENT, fontSize = 15.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        // the one action at the trailing end, its middle four under the name's
        Box(
            Modifier.align(Alignment.TopEnd).offset(y = ITEM_NAME + 4.dp - PILL_BUTTON / 2)
                .width(pillWidth).height(PILL_BUTTON)
                .clip(RoundedCornerShape(PILL_BUTTON / 2))
                .background(if (filled) ACCENT else PILL_STATE_FILL)
                .clickable(enabled = acts, onClick = { act() }),
            contentAlignment = Alignment.Center,
        ) {
            Text(label, color = if (filled) TITLE else ROW_INK, fontSize = 15.sp, fontWeight = FontWeight.SemiBold)
        }
    }
    Spacer(Modifier.height(16.dp))
    // what taking it means, in words, with the small glyph before it
    Row(Modifier.fillMaxWidth()) {
        InfoGlyph(Modifier.padding(start = 1.dp, top = 3.dp))
        Spacer(Modifier.width(9.dp))
        Text(caveat(item.kind), color = ROW_INK, fontSize = 15.sp)
    }
    Spacer(Modifier.height(20.dp))
    // the first picture the catalogue gave, filling the page's own width as the rest of it does
    val picture = pictureFor(item.previews.firstOrNull().orEmpty(), front)
    if (picture != null) {
        BoxWithConstraints(Modifier.fillMaxWidth()) {
            Image(
                bitmap = picture.asImageBitmap(),
                contentDescription = null,
                contentScale = ContentScale.Crop,
                modifier = Modifier.fillMaxWidth().height(maxWidth * ITEM_PICTURE_SHARE),
            )
        }
        Spacer(Modifier.height(20.dp))
    }
    // the facts, in columns ruled above and below
    val facts = factsOf(item)
    if (facts.isNotEmpty()) {
        FactsStrip(facts)
        Spacer(Modifier.height(24.dp))
    }
    // its words: the longer description where the catalogue gave one, else the short one
    val words = item.body.ifEmpty { item.description }
    if (words.isNotEmpty()) {
        Text(words, color = ROW_INK, fontSize = 16.sp)
        Spacer(Modifier.height(40.dp))
    }
    // the details, its title on the row content's own 16 in from the page edge
    Box(Modifier.fillMaxWidth().height(HEADING_LINE)) {
        Text(
            "Details", color = TITLE, fontSize = SECTION, fontWeight = FontWeight.Bold,
            modifier = Modifier.align(Alignment.TopStart).padding(start = 16.dp),
        )
    }
    Spacer(Modifier.height(HEADING_BELOW))
    DetailsCard(item)
    // more from the same maker, three across
    val more = front.items.filter { it.publisher == item.publisher && it.id != item.id }.take(3)
    if (more.isNotEmpty()) {
        Spacer(Modifier.height(40.dp))
        Box(Modifier.fillMaxWidth().height(HEADING_LINE)) {
            Text(
                "More from ${item.publisher}",
                color = TITLE, fontSize = SECTION, fontWeight = FontWeight.Bold,
                modifier = Modifier.align(Alignment.TopStart).padding(start = 16.dp),
            )
        }
        Spacer(Modifier.height(HEADING_BELOW))
        MoreFrom(more, front, onItem)
        Spacer(Modifier.height(22.dp))
    }
}

/** The facts in columns, ruled above and below and between, as the desktop's strip has them. */
@Composable
private fun FactsStrip(facts: List<Pair<String, String>>) {
    Box(Modifier.fillMaxWidth().height(FACTS_HEIGHT)) {
        Row(Modifier.fillMaxSize()) {
            facts.forEachIndexed { at, (label, value) ->
                Box(Modifier.weight(1f).fillMaxHeight()) {
                    if (at > 0) {
                        Box(
                            Modifier.align(Alignment.CenterStart).width(1.dp).fillMaxHeight()
                                .padding(vertical = 12.dp).background(SEP),
                        )
                    }
                    // the first column shares the row content's 16, so the strip has one left edge
                    val inset = 16.dp
                    // a header's rhythm inside the strip too: 16 over the label, 8 under it
                    Text(
                        label, color = IDLE, fontSize = 11.sp, fontWeight = FontWeight.SemiBold,
                        modifier = Modifier.offset(x = inset, y = 16.dp),
                    )
                    Text(
                        value, color = TITLE, fontSize = 16.sp, fontWeight = FontWeight.SemiBold,
                        maxLines = 1, overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.offset(x = inset, y = 41.dp).fillMaxWidth().padding(end = 10.dp),
                    )
                }
            }
        }
        Box(Modifier.fillMaxWidth().height(1.dp).background(SEP))
        Box(Modifier.align(Alignment.BottomStart).fillMaxWidth().height(1.dp).background(SEP))
    }
}

/** Whose it is: a card of rows, the label at one end and the value at the other. */
@Composable
private fun DetailsCard(item: StoreItem) {
    val rows = listOf("Category" to item.category, "Published by" to item.publisher)
    Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(GROUP_CORNER)).background(GROUP_FILL)) {
        rows.forEachIndexed { at, (label, value) ->
            if (at > 0) Box(Modifier.fillMaxWidth().height(1.dp).background(SEP))
            Row(
                Modifier.fillMaxWidth().height(GROUP_ROW).padding(horizontal = 16.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(label, color = TITLE, fontSize = 17.sp, modifier = Modifier.weight(1f))
                Text(value, color = ROW_INK, fontSize = 17.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
    }
}

/** Up to three of the maker's other things, each opening its own page. */
@Composable
private fun MoreFrom(items: List<StoreItem>, front: StoreFront, onItem: (StoreItem) -> Unit) {
    BoxWithConstraints(Modifier.fillMaxWidth()) {
        val each = (maxWidth - MORE_GAP * 2) / 3
        Row(horizontalArrangement = Arrangement.spacedBy(MORE_GAP)) {
            for (item in items) {
                val picture = pictureFor(item.preview, front)
                Column(Modifier.width(each).clickable { onItem(item) }) {
                    if (picture != null) {
                        ArtPicture(picture, CARD_CORNER, Modifier.fillMaxWidth().height(each * 0.75f))
                    } else {
                        Box(
                            Modifier.fillMaxWidth().height(each * 0.75f)
                                .clip(RoundedCornerShape(CARD_CORNER)).background(TILE_FILL),
                            contentAlignment = Alignment.Center,
                        ) {
                            Icon(kindIcon(item.kind), contentDescription = null, tint = IDLE, modifier = Modifier.size(36.dp))
                        }
                    }
                    Box(Modifier.fillMaxWidth().height(MORE_FOOT)) {
                        Text(
                            item.name,
                            color = TITLE, fontSize = 16.sp, fontWeight = FontWeight.SemiBold,
                            maxLines = 1, overflow = TextOverflow.Ellipsis,
                            modifier = Modifier.offset(y = 12.dp).fillMaxWidth(),
                        )
                        // the line and the state share its row, so neither runs under the other
                        Row(
                            Modifier.offset(y = 20.dp).fillMaxWidth().height(PILL_STATE),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Text(
                                item.publisher,
                                color = ROW_INK, fontSize = 14.sp, maxLines = 1, overflow = TextOverflow.Ellipsis,
                                modifier = Modifier.weight(1f),
                            )
                            Spacer(Modifier.width(8.dp))
                            StatePill(front.standing(item))
                        }
                    }
                }
            }
        }
    }
}

/** The facts the strip carries: always the four the mock shows, then the requirement when given. */
private fun factsOf(item: StoreItem): List<Pair<String, String>> {
    val facts = mutableListOf(
        "Size" to sizeText(item.bytes),
        "Version" to item.version.ifEmpty { "Not given" },
        "Licence" to item.licence.ifEmpty { "Not given" },
        "Tags" to if (item.tags.isEmpty()) "None" else item.tags.joinToString(", "),
    )
    if (item.requires.isNotEmpty()) facts += "Requires" to item.requires
    return facts
}

/**
 * Settings, as settings_page draws it: a small heading over a card, the card's rows, and the line
 * under it saying what the group means.
 */
@Composable
private fun SettingsPage(front: StoreFront, onSource: (SourceInfo) -> Unit, onAdd: () -> Unit) {
    PageTitle("Settings")
    Spacer(Modifier.height(24.dp))
    // what the store reads, one row each, and the way to add one
    GroupHead("SOURCES")
    Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(GROUP_CORNER)).background(GROUP_FILL)) {
        front.sources.forEachIndexed { at, source ->
            if (at > 0) GroupRule()
            Row(
                Modifier.fillMaxWidth().height(GROUP_ROW).clickable { onSource(source) }.padding(horizontal = 16.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column(Modifier.weight(1f)) {
                    Text(source.name, color = if (source.on) TITLE else IDLE, fontSize = 17.sp)
                    Text(sourceLine(source), color = ROW_INK, fontSize = 15.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
                // a disclosure chevron: the app's own glyph at 13, its trailing edge 16 in from the
                // row's edge (the row's own 16 of padding), its middle on the row's
                Icon(PcIcons.ChevronRight, contentDescription = null, tint = IDLE, modifier = Modifier.size(13.dp))
            }
        }
        if (front.sources.isNotEmpty()) GroupRule()
        Row(
            Modifier.fillMaxWidth().height(GROUP_ROW).clickable(onClick = onAdd).padding(horizontal = 16.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text("Add Source...", color = ACCENT, fontSize = 17.sp)
        }
    }
    GroupNote("The store reads these when it opens. Turning one off hides its items; nothing is uninstalled.")
    // when they were last read, which is the only checking there is so far
    GroupHead("UPDATES")
    RowsCard(listOf("Last Checked" to readText(front.sources.map { it.read }.maxOrNull() ?: 0uL, withYear = true)))
    GroupNote("The store reads its sources when it opens. Nothing is checked in the background.")
    // what came with the app, and what the store installed
    GroupHead("STORAGE")
    val came = front.items.count { it.file.isEmpty() } + front.installed.size
    RowsCard(listOf("Installed Items" to "$came items", "Downloads" to "0 in progress"))
    GroupNote("Downloads stop when the store closes. Installed items live in your project.", last = true)
}

/** One source's own page: its switch, its address, when it was read, and what can be done with it. */
@Composable
private fun SourcePage(front: StoreFront, source: SourceInfo, onRefresh: () -> Unit, onRemove: () -> Unit) {
    val scope = rememberCoroutineScope()
    val bundled = source.address.isEmpty()
    val local = source.address.isNotEmpty() && !source.address.startsWith("http")
    var said by remember(source.id) { mutableStateOf<String?>(null) }

    Spacer(Modifier.height(24.dp))
    Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(GROUP_CORNER)).background(GROUP_FILL)) {
        Row(
            Modifier.fillMaxWidth().height(GROUP_ROW).clickable { scope.launch { front.setOn(source.id, !source.on) } }
                .padding(horizontal = 16.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text("Enabled", color = TITLE, fontSize = 17.sp, modifier = Modifier.weight(1f))
            StoreSwitch(source.on)
        }
        GroupRule()
        Row(
            Modifier.fillMaxWidth().height(GROUP_ROW).padding(horizontal = 16.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text("Address", color = TITLE, fontSize = 17.sp, modifier = Modifier.weight(1f))
            Text(
                if (bundled) "Bundled with ProCraft" else source.address,
                color = ROW_INK, fontSize = 17.sp, maxLines = 1, overflow = TextOverflow.Ellipsis,
            )
        }
        GroupRule()
        Row(
            Modifier.fillMaxWidth().height(GROUP_ROW).padding(horizontal = 16.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text("Last Refreshed", color = TITLE, fontSize = 17.sp, modifier = Modifier.weight(1f))
            Text(readText(source.read, withYear = true), color = ROW_INK, fontSize = 17.sp)
        }
    }
    GroupNote(
        if (source.failure.isNotEmpty()) source.failure
        else "Off hides this source's items from the store. Nothing is uninstalled.",
        danger = source.failure.isNotEmpty(),
    )
    // one row each for what can be done: a catalogue on this machine is read again, and any
    // source but the one that came with the app can go
    if (!bundled) {
        Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(GROUP_CORNER)).background(GROUP_FILL)) {
            if (local) {
                Row(
                    Modifier.fillMaxWidth().height(GROUP_ROW).clickable(onClick = onRefresh).padding(horizontal = 16.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text("Refresh Now", color = ACCENT, fontSize = 17.sp)
                }
                GroupRule()
            }
            Row(
                Modifier.fillMaxWidth().height(GROUP_ROW).clickable(onClick = onRemove).padding(horizontal = 16.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text("Remove This Source", color = Danger, fontSize = 17.sp)
            }
        }
        GroupNote(
            "Items you installed from this source stay installed, and their cards keep saying where they came from.",
            last = true,
        )
    }
    said?.let { Text(it, color = Danger, fontSize = 13.sp, modifier = Modifier.padding(start = 16.dp, top = 8.dp)) }
}

/** Downloads, as downloads_page draws it: what is arriving, what waited, and what finished. */
@Composable
private fun DownloadsPage(front: StoreFront, onItem: (StoreItem) -> Unit) {
    PageTitle("Downloads")
    Spacer(Modifier.height(8.dp))
    GroupHead("DOWNLOADING")
    Text(
        "Nothing downloading.",
        color = ROW_INK, fontSize = 15.sp, fontWeight = FontWeight.Medium,
        modifier = Modifier.padding(start = 16.dp),
    )
    Spacer(Modifier.height(HEADING_LINE))
    // what has a newer file waiting in the catalogue, then everything that arrived
    val waiting = front.installed.values.mapNotNull { one -> front.items.firstOrNull { it.id == one.id } }
        .filter { front.standing(it) == Standing.UPDATE }
    val finished = front.installed.values.mapNotNull { one -> front.items.firstOrNull { it.id == one.id } }
    if (waiting.isNotEmpty()) {
        GroupHead("WAITING FOR YOU")
        RecordsCard(waiting, front, onItem)
        Spacer(Modifier.height(24.dp))
    }
    if (finished.isNotEmpty()) {
        GroupHead("FINISHED")
        RecordsCard(finished, front, onItem)
    }
}

/** What arrived, one row each: its kind's tile, its name, when it came, and its state. */
@Composable
private fun RecordsCard(items: List<StoreItem>, front: StoreFront, onItem: (StoreItem) -> Unit) {
    Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(GROUP_CORNER)).background(GROUP_FILL)) {
        items.forEachIndexed { at, item ->
            if (at > 0) GroupRule()
            Row(
                Modifier.fillMaxWidth().height(ROW_HEIGHT).clickable { onItem(item) }.padding(horizontal = 16.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Box(
                    Modifier.size(RECORD_TILE).clip(RoundedCornerShape(RECORD_TILE_CORNER)).background(TILE_FILL),
                    contentAlignment = Alignment.Center,
                ) {
                    Icon(kindIcon(item.kind), contentDescription = null, tint = IDLE, modifier = Modifier.size(20.dp))
                }
                Column(Modifier.weight(1f).padding(start = 12.dp)) {
                    Text(item.name, color = TITLE, fontSize = 17.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    val when_ = readText(front.installed[item.id]?.at ?: 0uL)
                    Text(
                        if (item.version.isEmpty()) when_ else "Version ${item.version} - $when_",
                        color = ROW_INK, fontSize = 15.sp, maxLines = 1, overflow = TextOverflow.Ellipsis,
                    )
                }
                Spacer(Modifier.width(12.dp))
                StatePill(front.standing(item))
            }
        }
    }
}

/** A kind's own wall, three across, in the order the bar's menu asked for. */
@Composable
private fun SeeAllPage(front: StoreFront, kind: ItemKind, sort: SortBy, ascending: Boolean, onItem: (StoreItem) -> Unit) {
    PageTitle(sectionTitle(kind))
    Spacer(Modifier.height(24.dp))
    val items = remember(front.items, kind, sort, ascending) {
        sorted(front.items.filter { it.kind == kind }, sort, ascending)
    }
    if (items.isEmpty()) {
        Spacer(Modifier.height(24.dp))
        Text(
            "Nothing here yet.",
            color = ROW_INK, fontSize = 15.sp, fontWeight = FontWeight.Medium,
            textAlign = TextAlign.Center, modifier = Modifier.fillMaxWidth(),
        )
        return
    }
    BoxWithConstraints(Modifier.fillMaxWidth()) {
        val each = (maxWidth - WALL_GAP * 2) / 3
        Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(24.dp)) {
            for (row in items.chunked(3)) {
                Row(horizontalArrangement = Arrangement.spacedBy(WALL_GAP)) {
                    for (item in row) {
                        val picture = pictureFor(item.preview, front)
                        Column(Modifier.width(each).clickable { onItem(item) }) {
                            if (picture != null) {
                                ArtPicture(picture, CARD_CORNER, Modifier.fillMaxWidth().height(each * 0.75f))
                            } else {
                                Box(
                                    Modifier.fillMaxWidth().height(each * 0.75f)
                                        .clip(RoundedCornerShape(CARD_CORNER)).background(TILE_FILL),
                                    contentAlignment = Alignment.Center,
                                ) {
                                    Icon(kindIcon(item.kind), contentDescription = null, tint = IDLE, modifier = Modifier.size(40.dp))
                                }
                            }
                            // the name, the line under it, and the state on the same line
                            Box(Modifier.fillMaxWidth().height(WALL_FOOT)) {
                                Text(
                                    item.name,
                                    color = TITLE, fontSize = 16.sp, fontWeight = FontWeight.SemiBold,
                                    maxLines = 1, overflow = TextOverflow.Ellipsis,
                                    modifier = Modifier.offset(y = 12.dp).fillMaxWidth(),
                                )
                                // the line and the state share its row, so neither runs under the other
                                Row(
                                    Modifier.offset(y = 29.dp).fillMaxWidth().height(PILL_STATE),
                                    verticalAlignment = Alignment.CenterVertically,
                                ) {
                                    Text(
                                        secondLine(item),
                                        color = ROW_INK, fontSize = 14.sp, maxLines = 1, overflow = TextOverflow.Ellipsis,
                                        modifier = Modifier.weight(1f),
                                    )
                                    Spacer(Modifier.width(8.dp))
                                    StatePill(front.standing(item))
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/**
 * The menu the Filters pill opens, as filters_menu() lays it out: the kind, then the source, one
 * radio a line, 262 wide at the pill's trailing edge with its top 6 under it.
 */
@Composable
private fun FiltersMenu(
    front: StoreFront,
    kind: ItemKind?,
    from: String?,
    onKind: (ItemKind?) -> Unit,
    onFrom: (String?) -> Unit,
    onClose: () -> Unit,
) {
    Box(Modifier.fillMaxSize().clickable(interactionSource = remember { MutableInteractionSource() }, indication = null) { onClose() }) {
        Column(
            Modifier.align(Alignment.TopEnd).padding(top = FILTERS_MENU_TOP, end = INSET).width(MENU_W)
                .shadow(24.dp, RoundedCornerShape(MENU_CORNER))
                .clip(RoundedCornerShape(MENU_CORNER)).background(BG).padding(top = 6.dp, bottom = 6.dp)
                .clickable(interactionSource = remember { MutableInteractionSource() }, indication = null) { },
        ) {
            MenuHead("Kind")
            MenuPick("All kinds", kind == null) { onKind(null) }
            MenuPick("Plugins", kind == ItemKind.PLUGIN) { onKind(ItemKind.PLUGIN) }
            MenuPick("Brush sets", kind == ItemKind.BRUSH_SET) { onKind(ItemKind.BRUSH_SET) }
            MenuPick("Asset packs", kind == ItemKind.ASSET_PACK) { onKind(ItemKind.ASSET_PACK) }
            MenuHead("Source")
            MenuPick("All sources", from == null) { onFrom(null) }
            for (source in front.sources) {
                MenuPick(source.name, from == source.id) { onFrom(source.id) }
            }
        }
    }
}

/** The menu the wall's bar opens: what to sort by, and which way up, one radio a line. */
@Composable
private fun SortMenu(
    sort: SortBy,
    ascending: Boolean,
    onSort: (SortBy) -> Unit,
    onAscending: (Boolean) -> Unit,
    onClose: () -> Unit,
) {
    Box(Modifier.fillMaxSize().clickable(interactionSource = remember { MutableInteractionSource() }, indication = null) { onClose() }) {
        Column(
            // hanging 6 under the sort button in the bar, as the desktop anchors it
            Modifier.align(Alignment.TopEnd).padding(top = MENU_TOP, end = INSET).width(MENU_W)
                .shadow(24.dp, RoundedCornerShape(MENU_CORNER))
                .clip(RoundedCornerShape(MENU_CORNER)).background(BG).padding(top = 6.dp, bottom = 6.dp)
                .clickable(interactionSource = remember { MutableInteractionSource() }, indication = null) { },
        ) {
            MenuHead("Sort by")
            MenuPick("Title", sort == SortBy.TITLE) { onSort(SortBy.TITLE) }
            MenuPick("Author", sort == SortBy.AUTHOR) { onSort(SortBy.AUTHOR) }
            MenuPick("Size", sort == SortBy.SIZE) { onSort(SortBy.SIZE) }
            MenuHead("Order")
            MenuPick("Ascending", ascending) { onAscending(true) }
            MenuPick("Descending", !ascending) { onAscending(false) }
        }
    }
}

@Composable
private fun MenuHead(text: String) {
    Box(Modifier.fillMaxWidth().height(MENU_HEAD)) {
        Text(text, color = IDLE, fontSize = 13.sp, modifier = Modifier.offset(x = 18.dp, y = 8.dp))
    }
}

@Composable
private fun MenuPick(label: String, on: Boolean, onClick: () -> Unit) {
    Box(Modifier.fillMaxWidth().height(MENU_ROW).clickable(onClick = onClick)) {
        Text(
            label, color = if (on) TITLE else ROW_INK, fontSize = 16.sp,
            modifier = Modifier.align(Alignment.CenterStart).padding(start = 18.dp),
        )
        Canvas(Modifier.align(Alignment.CenterEnd).padding(end = 18.dp).size(17.dp)) {
            drawCircle(if (on) ACCENT else IDLE, radius = size.minDimension / 2, style = Stroke(1.5.dp.toPx()))
            if (on) drawCircle(ACCENT, radius = size.minDimension / 2 - 4.dp.toPx())
        }
    }
}

/** Adding a source: one field on a panel over the sheet, as the desktop's own add sheet is. */
@Composable
private fun AddSheet(front: StoreFront, onClose: () -> Unit) {
    val scope = rememberCoroutineScope()
    var address by remember { mutableStateOf("") }
    var reason by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    val ready = address.isNotBlank() && !busy

    fun add() {
        if (!ready) return
        busy = true
        reason = null
        scope.launch {
            val why = front.add(address.trim())
            busy = false
            if (why == null) onClose() else reason = why
        }
    }

    Box(Modifier.fillMaxSize().background(Color.Black.copy(alpha = 0.4f)), contentAlignment = Alignment.Center) {
        Column(
            Modifier.widthIn(max = ADD_W).heightIn(max = ADD_H).fillMaxWidth().fillMaxHeight()
                .clip(RoundedCornerShape(ADD_CORNER)).background(BG),
        ) {
            // its bar: Cancel, the title, and Add
            Row(Modifier.fillMaxWidth().height(BAR_HEIGHT), verticalAlignment = Alignment.CenterVertically) {
                Box(Modifier.height(BAR_HEIGHT).clickable(onClick = onClose).padding(horizontal = 16.dp), contentAlignment = Alignment.Center) {
                    Text("Cancel", color = ACCENT, fontSize = 17.sp)
                }
                Text(
                    "Add Source", color = TITLE, fontSize = 17.sp, fontWeight = FontWeight.SemiBold,
                    textAlign = TextAlign.Center, modifier = Modifier.weight(1f),
                )
                Box(
                    Modifier.height(BAR_HEIGHT).clickable(enabled = ready, onClick = { add() }).padding(horizontal = 16.dp),
                    contentAlignment = Alignment.Center,
                ) {
                    Text("Add", color = if (ready) ACCENT else IDLE, fontSize = 17.sp, fontWeight = FontWeight.SemiBold)
                }
            }
            Box(Modifier.fillMaxWidth().height(1.dp).background(SEP))
            // a grouped list's header rhythm: 16 over ADDRESS, 8 between it and the field
            // the field's text starts 16 in from the panel edge, so the label starts there too
            Box(Modifier.padding(start = 16.dp)) { GroupHead("ADDRESS") }
            BasicTextField(
                value = address,
                onValueChange = { address = it },
                singleLine = true,
                // an address is not prose: left to the keyboard's suggestions a typed path comes
                // back with its words spelled out and its slashes spaced
                keyboardOptions = KeyboardOptions(
                    autoCorrectEnabled = false,
                    keyboardType = KeyboardType.Uri,
                    imeAction = ImeAction.Done,
                ),
                textStyle = TextStyle(color = TITLE, fontSize = 17.sp),
                cursorBrush = SolidColor(ACCENT),
                modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp).height(ADD_FIELD)
                    .clip(RoundedCornerShape(10.dp)).background(GROUP_FILL).padding(horizontal = 16.dp),
                decorationBox = { field ->
                    Box(contentAlignment = Alignment.CenterStart) {
                        if (address.isEmpty()) Text("https://", color = HINT, fontSize = 17.sp)
                        field()
                    }
                },
            )
            Spacer(Modifier.height(12.dp))
            Text(
                reason ?: "A catalogue is a JSON file. Its address can be a web address, or the path to a file on this machine.",
                color = if (reason != null) Danger else ROW_INK,
                fontSize = 13.sp,
                modifier = Modifier.padding(horizontal = 16.dp),
            )
        }
    }
}

/** The store's own switch: a 51 by 31 track and a round knob, as the desktop draws it. */
@Composable
private fun StoreSwitch(on: Boolean) {
    Box(
        Modifier.width(SWITCH_W).height(SWITCH_H).clip(RoundedCornerShape(16.dp))
            .background(if (on) ACCENT else SWITCH_OFF),
    ) {
        Box(
            Modifier.align(if (on) Alignment.CenterEnd else Alignment.CenterStart)
                .padding(horizontal = 2.dp).size(27.dp).clip(CircleShape).background(Color.White),
        )
    }
}

/** The page's own title, 34 bold, 12 under the bar, as the list and the settings page have it. */
@Composable
private fun PageTitle(text: String) {
    Box(Modifier.fillMaxWidth().padding(top = 12.dp).height(TITLE_LINE), contentAlignment = Alignment.CenterStart) {
        Text(text, color = TITLE, fontSize = TITLE_SIZE, fontWeight = FontWeight.Bold)
    }
}

/** A small heading over a card: 13, quiet, 16 above it and 8 under it, as a grouped list's is. */
@Composable
private fun GroupHead(text: String, inset: Boolean = true) {
    Column(Modifier.fillMaxWidth()) {
        Spacer(Modifier.height(GROUP_HEAD_ABOVE))
        Text(
            text, color = ROW_INK, fontSize = 13.sp,
            modifier = Modifier.height(GROUP_HEAD_LINE).fillMaxWidth().offset(x = if (inset) 16.dp else 0.dp),
        )
        Spacer(Modifier.height(GROUP_HEAD_BELOW))
    }
}

/** The line under a group: 16 under its card, and 52 tall, or 26 where it closes the page. */
@Composable
private fun GroupNote(text: String, danger: Boolean = false, last: Boolean = false) {
    val tail = if (last) GROUP_FOOT else GROUP_FOOT + 26.dp
    Box(Modifier.fillMaxWidth().height(GROUP_HEAD + tail)) {
        Text(
            text, color = if (danger) Danger else ROW_INK, fontSize = 13.sp,
            modifier = Modifier.padding(start = 16.dp, end = 16.dp).offset(y = GROUP_HEAD),
        )
    }
}

/** A card of rows: the label at one end, the value at the other, ruled between. */
@Composable
private fun RowsCard(rows: List<Pair<String, String>>) {
    Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(GROUP_CORNER)).background(GROUP_FILL)) {
        rows.forEachIndexed { at, (label, value) ->
            if (at > 0) GroupRule()
            Row(
                Modifier.fillMaxWidth().height(GROUP_ROW).padding(horizontal = 16.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(label, color = TITLE, fontSize = 17.sp, modifier = Modifier.weight(1f))
                Text(value, color = ROW_INK, fontSize = 17.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
    }
}

/** The rule a card draws above every row but its first: inset 16 in from both content edges. */
@Composable
private fun GroupRule() {
    Box(Modifier.fillMaxWidth().padding(horizontal = GROUP_RULE_INSET).height(1.dp).background(SEP))
}

/** What a source's row says under its name: its failure, where it came from, or when it was read. */
private fun sourceLine(source: SourceInfo): String {
    if (source.failure.isNotEmpty()) return source.failure
    if (source.address.isEmpty()) return "Bundled with ProCraft"
    val who = if (source.by.isEmpty()) source.address else source.by
    if (source.read == 0uL) return "$who - not read yet"
    return "$who - ${readText(source.read)}"
}

/** The heading a kind's own wall carries, and the section that opens it. */
private fun sectionTitle(kind: ItemKind): String = when (kind) {
    ItemKind.BRUSH_SET -> "Brushes"
    ItemKind.PLUGIN -> "Plugins"
    ItemKind.ASSET_PACK -> "Asset packs"
}

/** The wall's order, as the desktop sorts it: a field's keys, then up or down. */
private fun sorted(items: List<StoreItem>, sort: SortBy, ascending: Boolean): List<StoreItem> {
    val ordered = when (sort) {
        SortBy.TITLE -> items.sortedBy { it.name.lowercase() }
        SortBy.AUTHOR -> items.sortedWith(compareBy({ it.publisher.lowercase() }, { it.name.lowercase() }))
        SortBy.SIZE -> items.sortedBy { it.bytes }
    }
    return if (ascending) ordered else ordered.reversed()
}

/**
 * The store's install for a brush set: its file fetched, the digest checked against the catalogue's,
 * then handed to the app's own brush importer, and recorded. Null when it is installed, or the
 * sentence saying why it is not.
 */
private suspend fun installBrushSet(store: Store?, item: StoreItem, app: Application): String? {
    if (store == null) return "The store isn't open."
    if (item.kind != ItemKind.BRUSH_SET) return "This screen installs brush sets only."
    val fetched = withContext(Dispatchers.IO) { store.fetchItem(item.id) }
    if (fetched.failure.isNotEmpty()) return fetched.failure
    if (fetched.bytes.isEmpty()) return "The file came back empty."
    val digest = withContext(Dispatchers.Default) { digestOf(fetched.bytes) }
    if (!digest.equals(item.sha256, ignoreCase = true)) return "The file doesn't match the catalogue, so nothing was kept."
    val file = withContext(Dispatchers.IO) {
        runCatching {
            val dir = File(app.cacheDir, "store").apply { mkdirs() }
            File(dir, fileName(item.id, CanvasViewModel.BRUSH_SET_EXTENSION)).also { it.writeBytes(fetched.bytes) }
        }.onFailure { AppLog.e("store", "writing an item's file", it) }.getOrNull()
    } ?: return "There isn't enough room on this device for this file."
    val kept = try {
        keepBrushesForLater(app, file, "${item.name}.${CanvasViewModel.BRUSH_SET_EXTENSION}")
    } finally {
        file.delete()
    }
    return when (kept) {
        KeptBrush.Waiting -> {
            withContext(Dispatchers.IO) { store.keepInstalled(item.id, digest) }
            // the open canvas reads it in now, as the desk's store installs into its own library;
            // with no canvas open the file waits for the next one, as before
            StoreSheet.takeKeptBrushes?.invoke()
            null
        }
        KeptBrush.Empty -> "The file holds no brushes."
        KeptBrush.NoRoom -> CanvasViewModel.NO_ROOM_TO_IMPORT
        KeptBrush.Stopped -> "Nothing was installed."
    }
}

/** A file's name from an item's id, and the ending the app knows the kind by. */
private fun fileName(id: String, extension: String): String =
    id.map { if (it.isLetterOrDigit() || it in "-_.") it else '_' }.joinToString("").ifEmpty { "item" } + ".$extension"

/** The sha256 of [bytes], lowercase hex, as a catalogue writes it. */
private fun digestOf(bytes: ByteArray): String =
    MessageDigest.getInstance("SHA-256").digest(bytes).joinToString("") { "%02x".format(it) }

/** The kind's words: the same ones the desktop's item rows use. */
private fun kindText(kind: ItemKind): String = when (kind) {
    ItemKind.BRUSH_SET -> "Brush set"
    ItemKind.PLUGIN -> "Plugin"
    ItemKind.ASSET_PACK -> "Asset pack"
}

/** The glyph a kind's tile and cards draw while the catalogue has given no picture. */
private fun kindIcon(kind: ItemKind) = when (kind) {
    ItemKind.BRUSH_SET -> PcIcons.Brush
    ItemKind.PLUGIN -> PcIcons.Actions
    ItemKind.ASSET_PACK -> PcIcons.FolderFill
}

/** The line under a name: whose it is and how big, or that it came with the app. */
private fun secondLine(item: StoreItem): String =
    if (item.file.isEmpty()) "Bundled with ProCraft" else "${item.publisher} - ${sizeText(item.bytes)}"

/** A size the way the desktop writes one: whole KB, rounded up, or MB to a tenth. */
private fun sizeText(bytes: ULong): String =
    if (bytes >= 1uL shl 20) {
        String.format(Locale.ENGLISH, "%.1f MB", bytes.toDouble() / (1 shl 20))
    } else {
        "${(bytes + 1023uL) / 1024uL} KB"
    }

/** What the kind's page says about taking it, in the desktop's own words. */
private fun caveat(kind: ItemKind): String = when (kind) {
    ItemKind.BRUSH_SET -> "Adds to the brush library."
    ItemKind.PLUGIN -> "Runs code on your hooks. You can turn it off in Plugins, and nothing it draws or stores survives."
    ItemKind.ASSET_PACK -> "Adds to the asset library."
}

private val readableDay = DateTimeFormatter.ofPattern("d MMM", Locale.ENGLISH)
private val readableDayYear = DateTimeFormatter.ofPattern("d MMM yyyy", Locale.ENGLISH)

/** When a source was last read: the day, with the year where the desktop shows one. */
private fun readText(seconds: ULong, withYear: Boolean = false): String {
    if (seconds == 0uL) return "Never"
    val when_ = Instant.ofEpochSecond(seconds.toLong()).atZone(ZoneId.systemDefault())
    return when_.format(if (withYear) readableDayYear else readableDay)
}
