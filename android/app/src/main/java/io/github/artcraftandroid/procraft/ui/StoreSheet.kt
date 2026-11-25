package io.github.artcraftandroid.procraft.ui

import android.app.Application
import android.graphics.Bitmap
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.PageSize
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.IntrinsicSize
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
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
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
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.clipToBounds
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
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
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
import io.github.artcraftandroid.procraft.core.CanvasSession
import io.github.artcraftandroid.procraft.core.Installed
import io.github.artcraftandroid.procraft.core.ItemKind
import io.github.artcraftandroid.procraft.core.SourceInfo
import io.github.artcraftandroid.procraft.core.Store
import io.github.artcraftandroid.procraft.core.StoreItem
import io.github.artcraftandroid.procraft.core.StorePluginReview
import io.github.artcraftandroid.procraft.core.StoreBrushResult
import io.github.artcraftandroid.procraft.core.Plugins
import io.github.artcraftandroid.procraft.core.storeGalleryEntries
import io.github.artcraftandroid.procraft.core.storeCoverKey
import io.github.artcraftandroid.procraft.core.storeFeaturedOrder
import io.github.artcraftandroid.procraft.core.storeIconKey
import io.github.artcraftandroid.procraft.core.StorePreview
import java.io.File
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.util.Locale
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Semaphore
import kotlinx.coroutines.sync.withPermit
import kotlinx.coroutines.withContext

/** The Store overlay and the catalogue state it keeps across openings. */
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
    internal var plugins: Plugins? = null
    internal var pluginsChanged: (() -> Unit)? = null
    internal var currentSession: (() -> CanvasSession?)? = null
    internal var queueVersion by mutableStateOf(0)
        private set

    internal fun brushesApplied(results: List<StoreBrushResult>) {
        if (results.isEmpty()) return
        results.forEach { result -> front?.setProblem(result.item, result.failure) }
        queueVersion++
    }

    private var front: StoreFront? = null

    fun show() {
        open = true
    }

    fun hide() {
        open = false
        front?.cancelPluginReview()
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
fun StoreOverlay(vm: CanvasViewModel) {
    // a just-installed set goes into the canvas that is up; once it is gone the file waits as before
    SideEffect {
        StoreSheet.takeKeptBrushes = vm::addPendingBrushes
        StoreSheet.plugins = vm.plugins
        StoreSheet.pluginsChanged = vm::pluginsChanged
        StoreSheet.currentSession = { vm.session }
    }
    DisposableEffect(Unit) {
        onDispose {
            StoreSheet.takeKeptBrushes = null
            StoreSheet.plugins = null
            StoreSheet.pluginsChanged = null
            StoreSheet.currentSession = null
        }
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
                StorePage(StoreSheet.front(dir))
            }
        }
    }
}

// The sheet, under desktop/src/store.rs's names, so the two can be read side by side.
internal val BG get() = Color(if (Pc.light) 0xFFF0F0F0 else 0xFF212121)
internal val TITLE get() = Color(if (Pc.light) 0xFF111111 else 0xFFFFFFFF)
internal val ROW_INK get() = Color(if (Pc.light) 0xA63C3C43 else 0x99EBEBF5)
private val HINT get() = Color(if (Pc.light) 0x613C3C43 else 0x61EBEBF5)
internal val IDLE get() = Color(if (Pc.light) 0x803C3C43 else 0x80EBEBF5)
private val SEARCH_FILL get() = Color(if (Pc.light) 0x1F767680 else 0x2E767680)
private val PILL_FILL get() = Color(if (Pc.light) 0x1F767680 else 0x38767680)
private val PILL_STATE_FILL = Color(0x47787880)
private val PILL_ON get() = Color(if (Pc.light) 0xFF1B1B1B else 0xFFEDEDED)
private val PILL_ON_INK get() = Color(if (Pc.light) 0xFFEDEDED else 0xFF1B1B1B)
internal val TILE_FILL get() = Color(if (Pc.light) 0x14000000 else 0x14FFFFFF)
internal val GROUP_FILL get() = Color(if (Pc.light) 0xFFFFFFFF else 0x0EFFFFFF)
internal val SEP get() = Color(if (Pc.light) 0x4A3C3C43 else 0x99545458)
internal val ACCENT get() = Pc.tinted(Color(if (Pc.light) 0xFF007AFF else 0xFF0A84FF))

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
private val FEATURE_SPACING = 14.dp
// the band's scrim: nearly clear where the picture starts to show, deep at its foot
private val SCRIM_TOP = Color(0x2A000000)
private val SCRIM_FOOT = Color(0xF0000000)
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
private val FADE_HEIGHT = 22.dp

// the item page, as item_page() draws it
private val ITEM_ICON = 120.dp
private val ITEM_ICON_CORNER = 27.dp

private val CHIP_KIND = 20.dp
private val PILL_BUTTON = 30.dp
private val PILL_BUTTON_SIDE = 18.dp
private val FACTS_HEIGHT = 68.dp
// the item page's picture block, as wide as the page: 377 of 520 tall, the desktop's ratio
private const val ITEM_PICTURE_SHARE = 377f / 520f
internal val GROUP_ROW = 56.dp
internal val GROUP_CORNER = 10.dp
private val MORE_GAP = 16.dp


// the pages behind the bar, as settings_page, source_page, downloads_page, see_all_page draw them
private val GROUP_HEAD = 16.dp
// a grouped list's header rhythm: 16 above the header, 8 between it and what it labels
private val GROUP_HEAD_ABOVE = 16.dp
private val GROUP_HEAD_LINE = 17.dp
private val GROUP_HEAD_BELOW = 8.dp
// the separator's inset from the leading and trailing content edge
internal val GROUP_RULE_INSET = 16.dp
private val GROUP_FOOT = 26.dp
private val SWITCH_W = 51.dp
private val SWITCH_H = 31.dp
private val SWITCH_OFF = Color(0x52787880)
private val RECORD_TILE = 34.dp
private val RECORD_TILE_CORNER = 8.dp
private val WALL_GAP = 20.dp

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
    PREVIEW("Preview"),
    UPDATE("Update"),
    INSTALLED("Installed"),
}

/** What the sheet shows: the list, one item, one source, the settings, the downloads, or a wall. */
private sealed interface SheetPage {
    data object Browse : SheetPage
    data class Item(val listing: Listing) : SheetPage
    data class Source(val id: String) : SheetPage
    data object Settings : SheetPage
    data object Downloads : SheetPage
    data class SeeAll(val kind: ItemKind) : SheetPage
}

/** How the wall orders what it shows, and which way up. */
private enum class SortBy { TITLE, AUTHOR, SIZE }

/** The same static cover serves the cards and the first gallery page. */
@Composable
private fun coverFor(item: StoreItem, front: StoreFront): Bitmap? {
    val key = remember(item) { storeCoverKey(item) }
    LaunchedEffect(key, front.imageVersion()) { front.wantCover(key, item) }
    return front.picture(key)
}

@Composable
private fun ArtPicture(bitmap: Bitmap, corner: Dp, modifier: Modifier = Modifier, scale: ContentScale = ContentScale.Crop) {
    Image(
        bitmap = bitmap.asImageBitmap(),
        contentDescription = null,
        contentScale = scale,
        modifier = modifier.clip(RoundedCornerShape(corner)).background(TILE_FILL),
    )
}

@Composable
private fun ItemArtwork(item: StoreItem, front: StoreFront, corner: Dp, glyph: Dp, modifier: Modifier, square: Boolean = false) {
    val hasIcon = square && (item.icon.isNotEmpty() || item.kind == ItemKind.PLUGIN)
    val icon = if (hasIcon) {
        val key = remember(item) { storeIconKey(item) }
        LaunchedEffect(key, front.imageVersion()) { front.wantCover(key, item, square = true) }
        front.picture(key)
    } else null
    val picture = icon ?: coverFor(item, front)
    if (picture != null) {
        ArtPicture(picture, corner, modifier, if (square) ContentScale.Fit else ContentScale.Crop)
    } else {
        Box(modifier.clip(RoundedCornerShape(corner)).background(TILE_FILL), contentAlignment = Alignment.Center) {
            Icon(kindIcon(item.kind), contentDescription = null, tint = IDLE, modifier = Modifier.size(glyph))
        }
    }
}

internal data class Listing(val source: String, val id: String)
private val StoreItem.listing get() = Listing(source, id)
private val Installed.listing get() = Listing(source, id)

/**
 * The store in the app's folder, read off the main thread: its sources, what they list and what
 * was installed, read again after anything changes them.
 */
internal class PendingStorePlugin(val item: StoreItem, val review: StorePluginReview) {
    val title = "Review " + review.name()
    val message = review.message()
    val action = if (review.isUpdate()) "Approve Update" else "Approve Install"
}

internal class StoreFront(private val dir: File) {
    var store by mutableStateOf<Store?>(null)
        private set
    var sources by mutableStateOf<List<SourceInfo>>(emptyList())
        private set
    var items by mutableStateOf<List<StoreItem>>(emptyList())
        private set
    var installed by mutableStateOf<Map<Listing, Installed>>(emptyMap())
        private set
    var queued by mutableStateOf<List<StoreItem>>(emptyList())
        private set
    val installing = mutableStateMapOf<Listing, StoreItem>()
    var pluginReview by mutableStateOf<PendingStorePlugin?>(null)
        private set
    private val problems = mutableStateMapOf<Listing, String>()
    private val installs = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)

    var failure by mutableStateOf<String?>(null)
        private set

    /** The store opened and read: opening reads every local catalogue again, as the desktop's does. */
    suspend fun open() {
        if (store != null) {
            read()
            return
        }
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
        val session = StoreSheet.currentSession?.invoke()
        val read = withContext(Dispatchers.IO) {
            runCatching {
                if (session == null) s.readBrushInstalls(File(dir.parentFile, "brushes").path) else s.syncBrushInstalls(session)
                Triple(s.sources(), s.items(), s.installed()) to s.queuedBrushes()
            }.onFailure { AppLog.e("store", "reading the store", it) }
        }
        read.onSuccess { (catalogue, pending) ->
            val (readSources, readItems, readInstalled) = catalogue
            sources = readSources
            items = readItems
            installed = readInstalled.associateBy { it.listing }
            queued = pending
        }
        failure = read.exceptionOrNull()?.let { "The store couldn't be read." }
    }

    /** Adds the catalogue at [address] (a web address or a file on the device): null, or why not. */
    suspend fun add(address: String, localNetwork: Boolean): String? {
        val s = store ?: return "The store isn't open."
        val reason = withContext(Dispatchers.IO) { s.addSourceWithLocalNetwork(address, localNetwork) }
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
        if (reason == null) clearPictures()
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
        if (!item.kind.supportsInstall()) return Standing.PREVIEW
        val one = installed[item.listing]
        val changed = one != null && item.sha256.isNotEmpty() && !one.digest.equals(item.sha256, ignoreCase = true)
        return when {
            changed -> Standing.UPDATE
            one != null || item.file.isEmpty() -> Standing.INSTALLED
            else -> Standing.GET
        }
    }

    fun problem(item: StoreItem): String? = problems[item.listing]

    fun setProblem(item: StoreItem, reason: String?) {
        if (reason == null) problems.remove(item.listing) else problems[item.listing] = reason
    }

    fun install(item: StoreItem) {
        if (!item.kind.supportsInstall()) return
        val opened = store ?: return
        if (installing.containsKey(item.listing) || queued.any { it.listing == item.listing }) return
        if (item.kind == ItemKind.PLUGIN) {
            reviewPlugin(opened, item)
            return
        }
        installing[item.listing] = item
        installs.launch {
            val result = runCatching {
                withContext(Dispatchers.IO) {
                    opened.queueBrushListing(item)
                }
            }
            val reason = result.getOrElse {
                AppLog.e("store", "installing a listing", it)
                "The file couldn't be installed."
            }
            setProblem(item, reason)
            read()
            installing.remove(item.listing)
            if (reason == null && item.kind == ItemKind.BRUSH_SET) StoreSheet.takeKeptBrushes?.invoke()
        }
    }

    private fun reviewPlugin(opened: Store, item: StoreItem) {
        if (pluginReview != null || installing.values.any { it.kind == ItemKind.PLUGIN }) return
        val plugins = StoreSheet.plugins
        if (plugins == null) {
            setProblem(item, "Plugins are unavailable.")
            return
        }
        installing[item.listing] = item
        installs.launch {
            val inspected = runCatching {
                withContext(Dispatchers.IO) { opened.reviewPluginListing(item, plugins) }
            }.onFailure { AppLog.e("store", "inspecting a plugin", it) }.getOrNull()
            installing.remove(item.listing)
            val review = inspected?.review
            if (review == null) {
                setProblem(item, inspected?.failure ?: "The plugin couldn't be inspected.")
            } else if (StoreSheet.open) {
                pluginReview = PendingStorePlugin(item, review)
            } else {
                review.destroy()
            }
        }
    }

    fun cancelPluginReview() {
        val waiting = pluginReview ?: return
        pluginReview = null
        waiting.review.destroy()
    }

    fun approvePlugin(waiting: PendingStorePlugin) {
        if (pluginReview !== waiting) return
        val opened = store ?: return
        pluginReview = null
        val item = waiting.item
        val review = waiting.review
        installing[item.listing] = item
        installs.launch {
            val reason = try {
                withContext(Dispatchers.IO) { opened.approvePluginListing(review) }
            } catch (error: Exception) {
                AppLog.e("store", "installing a reviewed plugin", error)
                "The plugin couldn't be installed."
            } finally {
                review.destroy()
            }
            setProblem(item, reason)
            if (reason == null) StoreSheet.pluginsChanged?.invoke()
            read()
            installing.remove(item.listing)
        }
    }

    /** The categories a catalogue may name, as the pill row lists them. */
    fun categories(): List<String> = store?.categories().orEmpty()

    private val pictures = mutableStateMapOf<String, Bitmap?>()
    private val recent = LinkedHashSet<String>()
    private val asking = mutableSetOf<Pair<Int, String>>()
    private var pictureVersion by mutableStateOf(0)
    private val slots = Semaphore(4)
    private val work = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)

    fun picture(address: String): Bitmap? = pictures[address]

    private fun clearPictures() {
        pictureVersion++
        pictures.clear()
        recent.clear()
    }

    fun wantCover(address: String, item: StoreItem, square: Boolean = false) {
        if (!square && item.preview.isEmpty() && item.kind != ItemKind.PLUGIN) return
        if (pictures.containsKey(address)) {
            recent.remove(address)
            recent.add(address)
            return
        }
        val opened = store ?: return
        val version = pictureVersion
        val request = version to address
        if (!asking.add(request)) return
        work.launch {
            val picture = withContext(Dispatchers.IO) {
                slots.withPermit {
                    runCatching {
                        (if (square) opened.fetchStoreIcon(item) else opened.fetchStoreCover(item))?.toStoreBitmap()
                    }.getOrNull()
                }
            }
            asking.remove(request)
            if (version == pictureVersion) {
                pictures[address] = picture
                recent.add(address)
                if (recent.size > 32) {
                    val old = recent.first()
                    recent.remove(old)
                    pictures.remove(old)
                }
            }
        }
    }

    fun imageVersion(): Int = pictureVersion
}

internal fun StorePreview.toStoreBitmap(): Bitmap =
    Bitmap.createBitmap(width.toInt(), height.toInt(), Bitmap.Config.ARGB_8888).apply {
        copyPixelsFromBuffer(java.nio.ByteBuffer.wrap(rgba))
    }

@Composable
private fun StorePage(front: StoreFront) {
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
    // what the sheet has to say itself: a reason, or the question before a source goes
    var said by remember { mutableStateOf<Pair<String, String>?>(null) }
    var removing by remember { mutableStateOf<SourceInfo?>(null) }

    LaunchedEffect(front, StoreSheet.queueVersion) { front.open() }
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
    val item = (page as? SheetPage.Item)?.let { at -> front.items.firstOrNull { it.listing == at.listing } }
    val source = (page as? SheetPage.Source)?.let { at -> front.sources.firstOrNull { it.id == at.id } }
    val showing: SheetPage = when (page) {
        is SheetPage.Item -> if (item != null) page else SheetPage.Browse
        is SheetPage.Source -> if (source != null) page else SheetPage.Browse
        else -> page
    }

    // back takes what is over the page first, then goes up one page, and from the list it leaves
    fun stepOut() {
        when {
            front.pluginReview != null -> front.cancelPluginReview()
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
                ) { go(SheetPage.Item(it.listing)) }
                // the pages under the bar: 40 in from the sides, 20 above the sheet's foot
                is SheetPage.Item -> PageScroll {
                    val open = item
                    if (open != null) ItemPage(
                        front, open,
                        onTell = { said = "Not installed" to it },
                        onItem = { other -> go(SheetPage.Item(other.listing)) },
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
                    DownloadsPage(front) { go(SheetPage.Item(it.listing)) }
                }
                is SheetPage.SeeAll -> SeeAllPage(front, showing.kind, sort, ascending, Modifier.weight(1f)) { go(SheetPage.Item(it.listing)) }
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
        Alert(
            value = front.pluginReview,
            title = { it.title },
            message = { it.message },
            action = front.pluginReview?.action ?: "Approve Install",
            onCancel = { front.cancelPluginReview() },
            onAction = front::approvePlugin,
            destructive = false,
        )
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
            .filter { kind == null || it.kind.browseGroup() == kind }
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
    val brushes = items.filter { it.kind.browseGroup() == ItemKind.BRUSH_SET }
    val plugins = items.filter { it.kind == ItemKind.PLUGIN }
    val packs = items.filter { it.kind == ItemKind.ASSET_PACK }
    // what the records hold, in the order they were taken, as the desktop puts it at the foot
    val took = front.installed.values.mapNotNull { one -> items.firstOrNull { it.listing == one.listing } }

    Column(Modifier.fillMaxWidth()) {
        Spacer(Modifier.height(LIST_TOP))
        if (banded) {
            FeaturedCarousel(items, front, onItem)
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

@Composable
private fun FeaturedCarousel(items: List<StoreItem>, front: StoreFront, onItem: (StoreItem) -> Unit) {
    val day by produceState(Instant.now().epochSecond / 86_400L) {
        while (true) {
            val now = Instant.now().epochSecond
            value = now / 86_400L
            delay(minOf(60L, 86_400L - now.mod(86_400L)) * 1_000L)
        }
    }
    val featured = remember(items, day) {
        storeFeaturedOrder(items.map { "${it.source}:${it.id}" }, day.toULong()).map { items[it.toInt()] }
    }
    androidx.compose.runtime.key(featured.map { it.listing }) {
        BoxWithConstraints(Modifier.fillMaxWidth()) {
            val fullCards = if (maxWidth >= 680.dp) 2 else 1
            val pageCount = (featured.size - fullCards + 1).coerceAtLeast(1)
            val cardWidth = if (pageCount == 1) {
                (maxWidth - FEATURE_SPACING * (featured.size - 1)) / featured.size
            } else if (fullCards == 2) {
                (maxWidth - FEATURE_SPACING * 2) / 2.4f
            } else {
                (maxWidth - FEATURE_SPACING) / 1.16f
            }
            val pager = rememberPagerState { featured.size }
            val scope = rememberCoroutineScope()
            Column {
                HorizontalPager(
                    state = pager,
                    modifier = Modifier.fillMaxWidth().clipToBounds(),
                    pageSize = PageSize.Fixed(cardWidth),
                    pageSpacing = FEATURE_SPACING,
                    userScrollEnabled = pageCount > 1,
                ) { index -> Band(featured[index], front, onItem) }
                if (pageCount > 1) {
                    val selectedPage = when {
                        !pager.canScrollBackward -> 0
                        !pager.canScrollForward -> pageCount - 1
                        else -> pager.currentPage.coerceAtMost(pageCount - 1)
                    }
                    Row(
                        Modifier.fillMaxWidth().selectableGroup(),
                        horizontalArrangement = Arrangement.Center,
                    ) {
                        repeat(pageCount) { page ->
                            val selected = selectedPage == page
                            Box(
                                Modifier.size(44.dp)
                                    .semantics { contentDescription = "Featured page ${page + 1} of $pageCount" }
                                    .selectable(selected, role = Role.Tab) {
                                        scope.launch { pager.animateScrollToPage(page) }
                                    },
                                contentAlignment = Alignment.Center,
                            ) {
                                Box(Modifier.size(if (selected) 14.dp else 6.dp, 6.dp)
                                    .background(if (selected) TITLE else IDLE, CircleShape))
                            }
                        }
                    }
                }
            }
        }
    }
}

/** The listing's cover and summary, opening its item page when tapped. */
@Composable
private fun Band(item: StoreItem, front: StoreFront, onItem: (StoreItem) -> Unit) {
    val shape = RoundedCornerShape(BAND_CORNER)
    val picture = coverFor(item, front)
    val titleInk = if (picture == null) TITLE else Color.White
    val secondaryInk = if (picture == null) ROW_INK else Color(0xFFE9E9EF)
    Box(
        Modifier.fillMaxWidth().height(BAND_HEIGHT).clip(shape).background(TILE_FILL)
            .border(1.dp, SEP, shape)
            .clickable(role = Role.Button, onClickLabel = "Open ${item.name}") { onItem(item) },
    ) {
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
                Modifier.matchParentSize()
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
        Column(
            Modifier.align(Alignment.BottomStart).fillMaxWidth().padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            Text(
                item.category.uppercase(), color = secondaryInk, fontSize = 11.sp, fontWeight = FontWeight.SemiBold,
                maxLines = 1, overflow = TextOverflow.Ellipsis,
            )
            Text(
                item.name, color = titleInk, fontSize = 23.sp, lineHeight = 27.sp, fontWeight = FontWeight.Bold,
                maxLines = 2, overflow = TextOverflow.Ellipsis,
            )
            if (item.description.isNotEmpty()) {
                Text(item.description, color = secondaryInk, fontSize = 14.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
            Row(
                Modifier.fillMaxWidth().padding(top = 4.dp).height(PILL_STATE),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                ItemArtwork(item, front, 6.dp, 14.dp, Modifier.size(24.dp), square = true)
                Text(
                    item.publisher, color = secondaryInk, fontSize = 13.sp,
                    maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f),
                )
                StatePill(front.standing(item))
            }
        }
    }
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

@Composable
private fun Shelf(items: List<StoreItem>, front: StoreFront, onItem: (StoreItem) -> Unit) {
    StoreCards(items.take(SHELF_CARDS), SHELF_CARDS, SHELF_GAP, front, onItem)
}

@Composable
private fun StoreCards(items: List<StoreItem>, columns: Int, gap: Dp, front: StoreFront, onItem: (StoreItem) -> Unit) {
    BoxWithConstraints(Modifier.fillMaxWidth()) {
        val width = ((maxWidth - gap * (columns - 1)) / columns).coerceAtLeast(156.dp)
        Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(gap)) {
            for (item in items) StoreCard(item, front, width, onItem)
        }
    }
}

@Composable
private fun StoreCard(item: StoreItem, front: StoreFront, width: Dp, onItem: (StoreItem) -> Unit) {
    Column(Modifier.width(width).clip(RoundedCornerShape(CARD_CORNER)).clickable { onItem(item) }) {
        ItemArtwork(item, front, CARD_CORNER, 36.dp, Modifier.fillMaxWidth().height(width * 0.75f))
        Row(
            Modifier.fillMaxWidth().padding(top = 10.dp, bottom = 4.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Column(Modifier.weight(1f)) {
                Text(item.name, color = TITLE, fontSize = 15.sp, lineHeight = 21.sp,
                    fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(secondLine(item), color = ROW_INK, fontSize = 13.sp, lineHeight = 18.sp,
                    maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
            StatePill(front.standing(item), compact = true)
        }
    }
}

/** One row: its kind's tile, the name, one line under it, and the state at the trailing end. */
@Composable
private fun ItemRow(item: StoreItem, front: StoreFront, ruled: Boolean, onItem: (StoreItem) -> Unit) {
    Box(Modifier.fillMaxWidth().height(ROW_HEIGHT).clickable { onItem(item) }) {
        if (ruled) Box(Modifier.fillMaxWidth().height(1.dp).background(SEP))
        Row(Modifier.fillMaxSize(), verticalAlignment = Alignment.CenterVertically) {
            ItemArtwork(item, front, TILE_CORNER, 24.dp, Modifier.size(TILE), square = true)
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
private fun StatePill(standing: Standing, modifier: Modifier = Modifier, compact: Boolean = false) {
    val update = standing == Standing.UPDATE
    val tick = !compact && standing == Standing.INSTALLED
    Row(
        modifier.height(PILL_STATE).clip(RoundedCornerShape(PILL_STATE / 2))
            .background(if (update) ACCENT else PILL_STATE_FILL).padding(horizontal = if (compact) 10.dp else 14.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (tick) {
            Icon(PcIcons.Tick, contentDescription = null, tint = ROW_INK, modifier = Modifier.size(15.dp))
            Spacer(Modifier.width(6.dp))
        }
        Text(
            standing.text,
            color = if (update) Color.White else if (standing == Standing.INSTALLED) ROW_INK else ACCENT,
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
    onTell: (String) -> Unit,
    onItem: (StoreItem) -> Unit,
) {
    val busy = front.installing.containsKey(item.listing)
    val waiting = front.queued.any { it.listing == item.listing }
    val problem = front.problem(item)
    val standing = front.standing(item)
    val isIn = standing != Standing.GET
    val update = standing == Standing.UPDATE
    // the button's word, in the desktop's order: on its way, then a problem, then an update
    val available = item.kind.supportsInstall()
    val label = when {
        !available -> "Preview only"
        busy && item.kind == ItemKind.PLUGIN -> "Working"
        busy -> "Installing"
        waiting -> "Queued"
        problem != null -> "Not installed"
        update -> "Update"
        isIn -> "Installed"
        else -> "Install"
    }
    // the accent while there is something to do, and the neutral fill when there is not
    val filled = available && problem == null && (!isIn || update || busy)
    val acts = available && !busy && !waiting && (problem != null || !isIn || update)
    fun act() {
        if (!acts) return
        if (problem != null) {
            // the reason only when it is asked for, in the store's own alert
            front.setProblem(item, null)
            onTell(problem)
            return
        }
        front.install(item)
    }

    Spacer(Modifier.height(24.dp))
    BoxWithConstraints(Modifier.fillMaxWidth()) {
        val compact = maxWidth < 480.dp
        val iconSize = if (compact) 72.dp else ITEM_ICON
        val action: @Composable () -> Unit = {
            Box(
                Modifier.height(PILL_BUTTON).clip(RoundedCornerShape(PILL_BUTTON / 2))
                    .background(if (filled) ACCENT else PILL_STATE_FILL)
                    .clickable(enabled = acts, role = Role.Button, onClick = { act() })
                    .padding(horizontal = PILL_BUTTON_SIDE),
                contentAlignment = Alignment.Center,
            ) {
                Text(label, color = if (filled) Color.White else ROW_INK, fontSize = 15.sp, fontWeight = FontWeight.SemiBold)
            }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(if (compact) 12.dp else 20.dp),
            verticalAlignment = Alignment.CenterVertically) {
            ItemArtwork(item, front, if (compact) 16.dp else ITEM_ICON_CORNER,
                if (compact) 34.dp else 56.dp, Modifier.size(iconSize), square = true)
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                    Text(item.name, color = TITLE, fontSize = if (compact) 23.sp else 30.sp, fontWeight = FontWeight.Bold,
                        maxLines = if (compact) 2 else 1, overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.weight(1f, fill = false))
                    if (!compact) ItemKindBadge(item.kind)
                }
                Text(item.publisher, color = ACCENT, fontSize = 15.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
                if (compact) {
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                        ItemKindBadge(item.kind)
                        action()
                    }
                }
            }
            if (!compact) action()
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
    ItemGallery(item, front)
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

@Composable
private fun ItemKindBadge(kind: ItemKind) {
    Box(Modifier.height(CHIP_KIND).clip(RoundedCornerShape(6.dp)).background(TILE_FILL).padding(horizontal = 7.dp),
        contentAlignment = Alignment.Center) {
        Text(kindText(kind).uppercase(), color = ROW_INK, fontSize = 11.sp, fontWeight = FontWeight.SemiBold)
    }
}

@Composable
private fun ItemGallery(item: StoreItem, front: StoreFront) {
    val gallery = remember(item) { storeGalleryEntries(item) }
    if (gallery.isEmpty()) return

    androidx.compose.runtime.key(item.listing) {
        val pager = rememberPagerState { gallery.size }
        val scope = rememberCoroutineScope()
        val cover = if (gallery.first().cover) coverFor(item, front) else null
        BoxWithConstraints {
            // Keep the next page just in view on wider sheets and preserve the shape of previews.
            val peek = if (gallery.size > 1 && maxWidth >= 560.dp) 40.dp else 0.dp
            HorizontalPager(
                state = pager,
                contentPadding = PaddingValues(end = peek),
                pageSpacing = if (peek > 0.dp) 12.dp else 0.dp,
            ) { page ->
                val entry = gallery[page]
                Box(
                    Modifier.fillMaxWidth().height((maxWidth - peek) * 0.75f)
                        .clip(RoundedCornerShape(CARD_CORNER)).background(TILE_FILL),
                    contentAlignment = Alignment.Center,
                ) {
                    if (entry.cover && cover != null) {
                        Image(cover.asImageBitmap(), "Cover", Modifier.fillMaxSize(), contentScale = ContentScale.Fit)
                    } else if (!entry.cover) {
                        StoreGalleryMedia(front.store, entry, front.imageVersion(), active = pager.settledPage == page)
                    }
                }
            }
        }
        if (gallery.size > 1) BoxWithConstraints(Modifier.fillMaxWidth()) {
            val visible = minOf(gallery.size, 9, (maxWidth / 24.dp).toInt().coerceAtLeast(1))
            val first = (pager.currentPage - visible / 2).coerceIn(0, gallery.size - visible)
            Row(
                Modifier.fillMaxWidth().selectableGroup(),
                horizontalArrangement = Arrangement.Center,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                for (page in first until first + visible) {
                    val selected = pager.currentPage == page
                    val more = (page == first && first > 0) ||
                        (page == first + visible - 1 && page < gallery.lastIndex)
                    Box(
                        Modifier.width(24.dp).height(44.dp)
                            .semantics { contentDescription = "Image " + (page + 1) + " of " + gallery.size }
                            .selectable(selected = selected, role = Role.Tab, onClick = {
                                scope.launch { pager.animateScrollToPage(page) }
                            }),
                        contentAlignment = Alignment.Center,
                    ) {
                        Box(Modifier.size(if (more && !selected) 4.dp else 6.dp)
                            .background(if (selected) TITLE else IDLE, CircleShape))
                    }
                }
            }
        }
        Spacer(Modifier.height(20.dp))
    }
}

/** The facts in columns, ruled above and below and between, as the desktop's strip has them. */
@Composable
private fun FactsStrip(facts: List<Pair<String, String>>) {
    Box(Modifier.fillMaxWidth()) {
        Row(Modifier.fillMaxWidth().heightIn(min = FACTS_HEIGHT).height(IntrinsicSize.Min)) {
            facts.forEachIndexed { at, (label, value) ->
                Box(Modifier.weight(1f).fillMaxHeight(), contentAlignment = Alignment.CenterStart) {
                    if (at > 0) {
                        Box(
                            Modifier.align(Alignment.CenterStart).width(1.dp).fillMaxHeight()
                                .padding(vertical = 12.dp).background(SEP),
                        )
                    }
                    Column(
                        Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp),
                        verticalArrangement = Arrangement.spacedBy(3.dp),
                    ) {
                        Text(
                            label, color = IDLE, fontSize = 11.sp, lineHeight = 14.sp, fontWeight = FontWeight.SemiBold,
                            maxLines = 1, overflow = TextOverflow.Ellipsis,
                        )
                        Text(
                            value, color = TITLE, fontSize = 16.sp, lineHeight = 20.sp, fontWeight = FontWeight.SemiBold,
                            maxLines = 1, overflow = TextOverflow.Ellipsis,
                        )
                    }
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

@Composable
private fun MoreFrom(items: List<StoreItem>, front: StoreFront, onItem: (StoreItem) -> Unit) {
    StoreCards(items, 3, MORE_GAP, front, onItem)
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
    StoreFormGroup {
        front.sources.forEachIndexed { at, source ->
            if (at > 0) GroupRule()
            StoreFormRow(source.name, subtitle = sourceLine(source), chevron = true,
                labelColor = if (source.on) TITLE else IDLE, onClick = { onSource(source) })
        }
        if (front.sources.isNotEmpty()) GroupRule()
        StoreFormRow("Add Source...", labelColor = ACCENT, onClick = onAdd)
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
            Row(
                Modifier.fillMaxWidth().height(GROUP_ROW).clickable(onClick = onRefresh).padding(horizontal = 16.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text("Refresh Now", color = ACCENT, fontSize = 17.sp)
            }
            GroupRule()
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
    val transfers = front.installing.values.distinctBy { it.listing }
    if (transfers.isEmpty()) {
        Text("Nothing downloading.", color = ROW_INK, fontSize = 15.sp, modifier = Modifier.padding(start = 16.dp))
    } else {
        RecordsCard(transfers, front, onItem, "Installing")
    }
    if (front.queued.isNotEmpty()) {
        Spacer(Modifier.height(HEADING_LINE))
        GroupHead("QUEUED FOR THE BRUSH LIBRARY")
        RecordsCard(front.queued, front, onItem, "Queued")
    }
    Spacer(Modifier.height(HEADING_LINE))
    // what has a newer file waiting in the catalogue, then everything that arrived
    val waiting = front.installed.values.mapNotNull { one -> front.items.firstOrNull { it.listing == one.listing } }
        .filter { front.standing(it) == Standing.UPDATE }
    val finished = front.installed.values.mapNotNull { one -> front.items.firstOrNull { it.listing == one.listing } }
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
private fun RecordsCard(items: List<StoreItem>, front: StoreFront, onItem: (StoreItem) -> Unit, status: String? = null) {
    Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(GROUP_CORNER)).background(GROUP_FILL)) {
        items.forEachIndexed { at, item ->
            if (at > 0) GroupRule()
            Row(
                Modifier.fillMaxWidth().height(ROW_HEIGHT).clickable { onItem(item) }.padding(horizontal = 16.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                ItemArtwork(item, front, RECORD_TILE_CORNER, 20.dp, Modifier.size(RECORD_TILE), square = true)
                Column(Modifier.weight(1f).padding(start = 12.dp)) {
                    Text(item.name, color = TITLE, fontSize = 17.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    val when_ = status ?: readText(front.installed[item.listing]?.at ?: 0uL)
                    Text(
                        if (item.version.isEmpty()) when_ else "Version ${item.version} - $when_",
                        color = ROW_INK, fontSize = 15.sp, maxLines = 1, overflow = TextOverflow.Ellipsis,
                    )
                }
                Spacer(Modifier.width(12.dp))
                if (status == null) StatePill(front.standing(item))
            }
        }
    }
}

/** A kind's own wall, three across, in the order the bar's menu asked for. */
@Composable
private fun SeeAllPage(
    front: StoreFront,
    kind: ItemKind,
    sort: SortBy,
    ascending: Boolean,
    modifier: Modifier,
    onItem: (StoreItem) -> Unit,
) {
    val listings = remember(front.items, kind, sort, ascending) {
        sorted(front.items.filter { it.kind.browseGroup() == kind }, sort, ascending)
    }
    BoxWithConstraints(modifier.fillMaxWidth().padding(horizontal = INSET)) {
        val columns = if (maxWidth < 360.dp) 1 else if (maxWidth < 540.dp) 2 else 3
        val rows = remember(listings, columns) { listings.chunked(columns) }
        val each = (maxWidth - WALL_GAP * (columns - 1)) / columns
        LazyColumn(Modifier.fillMaxWidth(), contentPadding = PaddingValues(bottom = 20.dp), verticalArrangement = Arrangement.spacedBy(24.dp)) {
            item { PageTitle(sectionTitle(kind)) }
            if (rows.isEmpty()) item {
                Text(
                    "Nothing here yet.", color = ROW_INK, fontSize = 15.sp, fontWeight = FontWeight.Medium,
                    textAlign = TextAlign.Center, modifier = Modifier.fillMaxWidth(),
                )
            }
            items(rows) { row ->
                Row(horizontalArrangement = Arrangement.spacedBy(WALL_GAP)) {
                    for (item in row) StoreCard(item, front, each, onItem)
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
            MenuPick("Brushes", kind == ItemKind.BRUSH_SET) { onKind(ItemKind.BRUSH_SET) }
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
    var localNetwork by remember { mutableStateOf(false) }
    var busy by remember { mutableStateOf(false) }
    val ready = address.isNotBlank() && !busy

    fun add() {
        if (!ready) return
        busy = true
        reason = null
        scope.launch {
            val why = front.add(address.trim(), localNetwork)
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
            if (address.trim().startsWith("https://")) {
                Row(Modifier.padding(horizontal = 16.dp), verticalAlignment = Alignment.CenterVertically) {
                    androidx.compose.material3.Checkbox(
                        checked = localNetwork,
                        onCheckedChange = { localNetwork = it },
                        enabled = !busy,
                    )
                    Text("Allow this HTTPS host on my local network", color = ROW_INK, fontSize = 13.sp)
                }
            }
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
internal fun StoreSwitch(on: Boolean, modifier: Modifier = Modifier) {
    Box(
        modifier.width(SWITCH_W).height(SWITCH_H).clip(RoundedCornerShape(16.dp))
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
internal fun GroupHead(text: String, inset: Boolean = true) {
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
internal fun GroupNote(text: String, danger: Boolean = false, last: Boolean = false) {
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
    StoreFormGroup {
        rows.forEachIndexed { at, (label, value) ->
            if (at > 0) GroupRule()
            StoreFormRow(label, value = value)
        }
    }
}

/** The rule a card draws above every row but its first: inset 16 in from both content edges. */
@Composable
internal fun GroupRule() {
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
private fun ItemKind.browseGroup(): ItemKind = when (this) {
    ItemKind.BRUSH, ItemKind.BRUSH_LIBRARY -> ItemKind.BRUSH_SET
    else -> this
}

private fun ItemKind.supportsInstall(): Boolean = this == ItemKind.BRUSH_SET || this == ItemKind.PLUGIN

private fun sectionTitle(kind: ItemKind): String = when (kind) {
    ItemKind.BRUSH_SET, ItemKind.BRUSH, ItemKind.BRUSH_LIBRARY -> "Brushes"
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

/** The kind's words: the same ones the desktop's item rows use. */
private fun kindText(kind: ItemKind): String = when (kind) {
    ItemKind.BRUSH_SET -> "Brush set"
    ItemKind.BRUSH -> "Brush"
    ItemKind.BRUSH_LIBRARY -> "Brush library"
    ItemKind.PLUGIN -> "Plugin"
    ItemKind.ASSET_PACK -> "Asset pack"
}

/** The glyph a kind's tile and cards draw while the catalogue has given no picture. */
private fun kindIcon(kind: ItemKind) = when (kind) {
    ItemKind.BRUSH_SET, ItemKind.BRUSH, ItemKind.BRUSH_LIBRARY -> PcIcons.Brush
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
    ItemKind.ASSET_PACK, ItemKind.BRUSH, ItemKind.BRUSH_LIBRARY -> "Browse the contents in the gallery."
}

private val readableDay = DateTimeFormatter.ofPattern("d MMM", Locale.ENGLISH)
private val readableDayYear = DateTimeFormatter.ofPattern("d MMM yyyy", Locale.ENGLISH)

/** When a source was last read: the day, with the year where the desktop shows one. */
private fun readText(seconds: ULong, withYear: Boolean = false): String {
    if (seconds == 0uL) return "Never"
    val when_ = Instant.ofEpochSecond(seconds.toLong()).atZone(ZoneId.systemDefault())
    return when_.format(if (withYear) readableDayYear else readableDay)
}
