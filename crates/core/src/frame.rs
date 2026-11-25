//! Drawing a frame into a window's image: by the render thread on Android and iPadOS, and by the
//! desktop app's own loop; and setting up the window's surface for it.

use std::time::{Duration, Instant};

use procraft_engine::geom::Vec2;
use procraft_engine::profile::Profile;
use procraft_gpu::{Backdrop, Gpu, SelectionLook, Stripes, Target, wgpu};

use crate::selection::SelectionDisplay;
use crate::{
    BACKDROP_SIGMA_PT, SELECTION_FILL, SELECTION_TINT, STRIPE_COVERAGE, STRIPE_PERIOD_PT, STRIPE_SPEED_PT, State,
};

impl State {
    /// Whether the window's surface asks for wide colour (see [`configure`]): only for a canvas
    /// whose colours go past sRGB (Display P3, CMYK shown through Display P3, an imported profile),
    /// and only while the app allows it. An sRGB canvas shows the same in either, so its window
    /// stays plain sRGB: the layer screens' compositors handle best (a wide one has blurred the
    /// interface over the canvas, or dulled the canvas while nothing moved, on some tablets).
    pub(crate) fn wide_colour(&self) -> bool {
        self.wide_colour_allowed && (self.doc.profile.is_p3() || self.doc.icc.is_some())
    }

    /// Draws the whole canvas fitted and centred on black into `view` (`format`, `size` pixels,
    /// its colours shown in `screen`): Project canvas, on a second display.
    pub(crate) fn draw_fitted(
        &mut self,
        view: &wgpu::TextureView,
        format: wgpu::TextureFormat,
        size: (u32, u32),
        screen: Profile,
    ) {
        let (Some(canvas), Some(gpu)) = (self.canvas.as_mut(), self.gpu.as_ref()) else { return };
        let to_screen = procraft_engine::view::View::fit((self.doc.width(), self.doc.height()), size).to_screen();
        let convert = colours_to(self.doc.profile, screen);
        let key = crate::colour::table_key(&self.doc, screen);
        if self.display_lut.as_ref().map(|(k, _)| *k) != key {
            self.display_lut = key
                .zip(crate::colour::display_table(&self.doc, screen))
                .map(|(k, table)| (k, canvas.display_lut(gpu, &table)));
        }
        let lut = self.display_lut.as_ref().map(|(_, l)| l);
        canvas.render(
            gpu,
            &mut self.doc,
            Some(Target {
                view,
                format,
                size,
                to_screen,
                surround: [0.0; 3],
                grid: None,
                selection: None,
                backdrop: None,
                convert,
                lut,
                // the second display's own density isn't known: crisp once a pixel covers more
                crisp_from: 1.0,
                look: None,
                traced: false,
            }),
        );
    }

    /// The bands over what is not selected, `started` being when drawing began.
    fn stripes_shown(&self, started: Instant) -> SelectionLook {
        let s = self.display_scale;
        let period = STRIPE_PERIOD_PT * s;
        let drift = (started.elapsed().as_secs_f32() * STRIPE_SPEED_PT * s) % period;
        SelectionLook::Stripes(Stripes { period, drift, coverage: STRIPE_COVERAGE * self.selection_visibility })
    }

    /// How the selection looks, `started` being when drawing began.
    fn selection_shown(&self, started: Instant, screen: Profile) -> SelectionLook {
        match self.selection_display {
            SelectionDisplay::Stripes => self.stripes_shown(started),
            SelectionDisplay::Tint => SelectionLook::Tint(Profile::Srgb.convert(SELECTION_TINT, screen)),
            SelectionDisplay::Fill => SelectionLook::Fill(Profile::Srgb.convert(SELECTION_FILL, screen)),
            // the paint colour is in the canvas's colours (a model shows it over what is selected)
            SelectionDisplay::ColourFill => SelectionLook::Fill(self.doc.profile.convert(self.color, screen)),
        }
    }

    /// When the selection's bands next show moved: when they have drifted a pixel (at 10 points a
    /// second, 30 frames a second on a 3x screen rather than every frame a 120 Hz screen makes).
    fn stripes_due(&self) -> Instant {
        let pixel = 1.0 / (STRIPE_SPEED_PT * self.display_scale.max(0.5));
        Instant::now() + Duration::from_secs_f32(pixel.clamp(1.0 / 240.0, 0.1))
    }

    /// Draws a frame into `view` (`format`, `size` window pixels, its colours shown in `screen`):
    /// the model open to paint on, the drawing pad as a sphere, or the canvas with its surround,
    /// grid, selection and frosted panels. `started` is when drawing began, for the bands' drift.
    /// Leaves `needs_frame` set while another frame is wanted.
    pub(crate) fn draw_frame(
        &mut self,
        view: &wgpu::TextureView,
        format: wgpu::TextureFormat,
        size: (u32, u32),
        screen: Profile,
        started: Instant,
    ) {
        self.lay_frame(view, format, size, screen, started);
        // and when it ends: a frame that took long (a weak GPU's, a third of a second) still counts
        // as one being drawn, so a read beside the frames keeps leaving them what they lay
        self.frame_drawn_at = Some(Instant::now());
    }

    fn lay_frame(
        &mut self,
        view: &wgpu::TextureView,
        format: wgpu::TextureFormat,
        size: (u32, u32),
        screen: Profile,
        started: Instant,
    ) {
        self.needs_frame = false;
        self.frame_due = None;
        self.frame_drawn_at = Some(Instant::now());
        // an HDR canvas's light above white, rolled off to what the screen shows now (white, on
        // a surface without extended range)
        if let Some(canvas) = self.canvas.as_mut() {
            let extended = format == wgpu::TextureFormat::Rgba16Float;
            canvas.set_display_peak(if extended { self.display_peak } else { 1.0 });
        }
        self.attach_media();
        // a vector layer's warp, its strokes bent through the mesh as it is now (one that takes
        // long to draw waits its turn, and asks for the frame after)
        if self.vector_warp_draw(false) {
            self.needs_frame = true;
        }
        // a shape moved since the last frame is drawn again where it is now, once however many
        // moves came meanwhile
        self.draw_moved_shape();
        // what runs ahead of the open stroke, once however many samples came since the last frame
        self.draw_ahead();
        // a fill's region as it has grown since, once however many samples came
        self.show_fill();
        // the view settling into place keeps drawing until it is there
        if self.step_view_motion() {
            self.needs_frame = true;
        }
        let to_screen = self.view.map(|v| v.to_screen());
        // plugins' layers drawn sharp over the canvas, zoomed in and settled
        if let Some(to_screen) = to_screen
            && !self.scene_shown()
            && self.sphere.is_none()
        {
            self.keep_sharp(size, to_screen);
        }
        let grid = self.surround_grid(Vec2::ZERO);
        let surround = self.surround();
        let selection_shown = self.selection_shown(started, screen);
        // Color Fill's colour shows on the canvas in the current layer's place, those above it
        // over it; bands while that layer can't be filled
        let fill_preview = self.colour_fill_preview();
        let canvas_selection = match self.selection_display {
            SelectionDisplay::ColourFill if fill_preview.is_some() => None,
            SelectionDisplay::ColourFill => Some(self.stripes_shown(started)),
            _ => Some(selection_shown),
        };
        // (a model or the drawing pad as a sphere shows the canvas as its layers are)
        let flat = !self.scene_shown() && self.sphere.is_none();
        if let Some(canvas) = self.canvas.as_mut() {
            canvas.set_fill_preview(fill_preview.filter(|_| flat));
            // the line where the selection fades out, while its feathering is dragged
            canvas.set_selection_edge(self.feather_edge && self.selection.is_some() && self.transform.is_none());
        }
        // while a transform is open the selection shows no bands: its box shows what moves
        let banded = self.transform.is_none();
        // vector layers drawn again as fine as the screen, once the view has rested zoomed in
        let crisp = if self.scene_shown() || self.sphere.is_some() {
            crate::crisp::CrispShown::No
        } else {
            self.crisp_frame(size, screen)
        };
        // (a lasso's fill shown as it is drawn shows there too)
        self.crisp_lasso();
        // what filter layers run, before they are drawn
        self.give_layer_filters();
        // a model is drawn in place of the flat canvas, and the drawing pad may be a sphere
        if self.scene_shown() {
            let convert = colours_to(self.doc.profile, screen);
            let selected = self.selection.is_some() && banded;
            let look = selected.then_some(selection_shown);
            // the bands drift, so they keep drawing, a frame each time they have moved a pixel
            if matches!(look, Some(SelectionLook::Stripes(_))) {
                self.frame_due = Some(self.stripes_due());
            }
            self.scene_selection_look(look);
            self.render_scene(view, format, size, convert);
            self.frames += 1;
        } else if self.sphere.is_some() {
            let convert = colours_to(self.doc.profile, screen);
            self.render_sphere(view, format, size, convert);
            self.frames += 1;
        } else if let (shown_look, Some(canvas), Some(gpu), Some(to_screen)) =
            (self.shown_look(), self.canvas.as_mut(), self.gpu.as_ref(), to_screen)
        {
            // the paper the plugins give, which a canvas made since hasn't taken yet
            canvas.set_paper(gpu, self.plugin_paper.as_ref());
            // and how much they dim the layers but the current one, likewise
            canvas.set_dim_others(self.plugin_dim);
            // and the picture round it, likewise
            crate::surround::put_on(canvas, gpu, self.surround_picture.as_deref());
            let s = self.display_scale;
            // the bands over what is not selected drift, so they keep drawing
            let look = canvas_selection.filter(|_| canvas.has_selection() && banded);
            let animating = matches!(look, Some(SelectionLook::Stripes(_)));
            let convert = colours_to(self.doc.profile, screen);
            // a canvas with a profile of its own shows through a table, made when that changes
            let key = crate::colour::table_key(&self.doc, screen);
            if self.display_lut.as_ref().map(|(k, _)| *k) != key {
                self.display_lut = key
                    .zip(crate::colour::display_table(&self.doc, screen))
                    .map(|(k, table)| (k, canvas.display_lut(gpu, &table)));
            }
            let lut = self.display_lut.as_ref().map(|(_, l)| l);
            let backdrop = (!self.backdrops.is_empty() || !self.backdrop_windows.is_empty()).then(|| Backdrop {
                regions: &self.backdrops,
                windows: &self.backdrop_windows,
                sigma: BACKDROP_SIGMA_PT * s,
            });
            // a dropped colour's fill spreading in: the canvas as it was over it, but for a hole
            match self.reveal.as_mut().map(|r| r.hole_at(Instant::now())) {
                Some(Some(hole)) => {
                    canvas.veil_hole(&hole);
                    self.needs_frame = true;
                }
                Some(None) => {
                    self.reveal = None;
                    canvas.veil_end();
                }
                None => {}
            }
            // (that picture's canvas in place of the artwork's, its pixels the window's; or, the
            // view having moved since it was made, over the artwork's, where that part of it now is)
            use crate::crisp::CrispShown;
            let over = match crisp {
                CrispShown::Over(map) => Some(map),
                _ => None,
            };
            let whole = crisp == CrispShown::Whole;
            let (canvas, doc, to_screen) = match self.crisp.as_mut().filter(|_| whole) {
                Some(sharp) => (&mut sharp.canvas, &mut sharp.doc, sharp.to_screen),
                None => (canvas, &mut self.doc, to_screen),
            };
            let look = (canvas.has_selection() && banded).then_some(selection_shown);
            canvas.render(
                gpu,
                doc,
                Some(Target {
                    view,
                    format,
                    size,
                    to_screen,
                    surround,
                    grid: Some(grid),
                    selection: look,
                    backdrop,
                    convert,
                    lut,
                    crisp_from: self.display_scale,
                    look: shown_look,
                    traced: true,
                }),
            );
            // caches still being remade: another frame soon, and the artwork isn't shown yet
            let busy = canvas.busy();
            // (a video's picture being read off this thread shows on the frame after it comes)
            if canvas.wants_render() || canvas.videos_pending() {
                self.needs_frame = true;
            } else if animating {
                self.frame_due = Some(self.stripes_due());
            }
            // counted only when the canvas was drawn, so a first frame means the artwork shows (and
            // its opening didn't end the app)
            if !busy {
                self.frames += 1;
                if let Some(mark) = self.open_mark.take() {
                    // (its file written off the frame's thread; on it if no thread can be made)
                    let _ = std::thread::Builder::new().name("open mark".into()).spawn(move || drop(mark));
                }
            }
            if let (Some(to_screen), Some(sharp)) = (over, self.crisp.as_mut()) {
                let (sharp, doc) = (&mut sharp.canvas, &mut sharp.doc);
                sharp.set_over(true);
                sharp.render(
                    gpu,
                    doc,
                    Some(Target {
                        view,
                        format,
                        size,
                        to_screen,
                        surround,
                        grid: None,
                        selection: None,
                        backdrop: None,
                        convert,
                        lut: None,
                        // (it is no longer a pixel to a pixel: smoothed, never squares)
                        crisp_from: f32::INFINITY,
                        look: shown_look,
                        traced: false,
                    }),
                );
                sharp.set_over(false);
            }
        }
        // the view resting: a frame when the sharp picture is to be made
        if let Some(due) = self.crisp_due {
            self.frame_due = Some(self.frame_due.map_or(due, |at| at.min(due)));
        }
        self.heed_filter_run();
        // a time-lapse frame asked for: taken once this frame has laid what it waited for (a model
        // or the pad as a sphere lays the canvas only as a read does: taken at once)
        let at_once = self.scene_shown() || self.sphere.is_some();
        self.take_lapse_picture(at_once);
    }
}

/// The least time between the beginnings of two frames for a screen showing at most `hertz` frames
/// a second: a tenth under its period, so a frame is ready for every refresh as frames drift
/// against it. None for a rate that can't be.
pub(crate) fn frame_spacing(hertz: f32) -> Option<Duration> {
    (hertz.is_finite() && hertz >= 1.0).then(|| Duration::from_secs_f32(0.9 / hertz.min(1000.0)))
}

/// The present mode a surface is configured with, of those it offers: Mailbox, where the screen
/// shows the newest frame at each refresh and drops any older one waiting (none queue up behind each
/// other), else Fifo.
fn present_mode(offered: &[wgpu::PresentMode]) -> wgpu::PresentMode {
    if offered.contains(&wgpu::PresentMode::Mailbox) { wgpu::PresentMode::Mailbox } else { wgpu::PresentMode::Fifo }
}

/// The frames a surface is configured to have in flight: one, the least (the GPU drawing one while
/// the screen shows the last).
const FRAME_LATENCY: u32 = 1;

/// The profile a configured surface shows colours in.
pub fn screen_profile(config: &wgpu::SurfaceConfiguration) -> Profile {
    match config.color_space {
        wgpu::SurfaceColorSpace::DisplayP3 | wgpu::SurfaceColorSpace::ExtendedDisplayP3 => Profile::DisplayP3,
        _ => Profile::Srgb,
    }
}

/// The matrix moving a canvas's colours in `from` to `to`, if they differ.
pub(crate) fn colours_to(from: Profile, to: Profile) -> Option<[[f32; 3]; 3]> {
    (from.is_p3() != to.is_p3()).then(|| from.matrix_to(to))
}

/// Configures the surface for `size`: a format that doesn't re-encode sRGB, the lowest-latency
/// present mode on offer, one frame in flight, and (if `wide`) Display P3 where the screen takes
/// it. A surface an sRGB interface is drawn on as it is (egui's) asks for no wide colour, or the
/// interface would show over-saturated. For an HDR canvas (`hdr`), half floats in the screen's
/// extended range where it has one (the iPad's EDR), so light above white shows brighter than
/// white. None if it can't be (yet).
pub fn configure(
    gpu: &Gpu,
    surface: &wgpu::Surface<'_>,
    size: (u32, u32),
    wide: bool,
    hdr: bool,
) -> Option<wgpu::SurfaceConfiguration> {
    let caps = surface.get_capabilities(&gpu.adapter);
    if hdr && let Some(config) = configure_extended(gpu, surface, &caps, size) {
        return Some(config);
    }
    configure_with(gpu, surface, &caps, size, wide)
}

/// [`configure`] with the surface's capabilities as asked before: they don't change while the
/// surface lasts, and asking for them can be slow (some systems try out a buffer of each format to
/// tell, each time they are asked).
pub(crate) fn configure_with(
    gpu: &Gpu,
    surface: &wgpu::Surface<'_>,
    caps: &wgpu::SurfaceCapabilities,
    size: (u32, u32),
    wide: bool,
) -> Option<wgpu::SurfaceConfiguration> {
    // an 8-bit one first: a float surface (Rgba16Float, offered first on some Linux drivers) is
    // read by the compositor as linear, so the sRGB-encoded colours drawn on it show washed out
    let eight_bit = [wgpu::TextureFormat::Bgra8Unorm, wgpu::TextureFormat::Rgba8Unorm];
    let format = eight_bit
        .into_iter()
        .find(|f| caps.formats.contains(f))
        .or_else(|| caps.formats.iter().copied().find(|f| !f.is_srgb()))
        .or_else(|| caps.formats.first().copied())?;

    // (a surface that presents no way at all can't be configured)
    caps.present_modes.first()?;
    let mut config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format,
        width: size.0.max(1),
        height: size.1.max(1),
        present_mode: present_mode(&caps.present_modes),
        desired_maximum_frame_latency: FRAME_LATENCY,
        alpha_mode: wgpu::CompositeAlphaMode::Auto,
        view_formats: Vec::new(),
        color_space: wgpu::SurfaceColorSpace::Auto,
    };
    if caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::Opaque) {
        config.alpha_mode = wgpu::CompositeAlphaMode::Opaque;
    }
    // wide colour where the screen takes it: the canvas is shown in Display P3
    let p3 = wide
        && caps
            .format_capabilities
            .iter()
            .any(|f| f.format == format && f.color_spaces.contains(wgpu::SurfaceColorSpaces::DISPLAY_P3));
    if p3 {
        config.color_space = wgpu::SurfaceColorSpace::DisplayP3;
    }
    // the iPad's images are shown in turn, one a refresh (there is no Mailbox there): with two, a
    // frame can only be begun once the one before shows, and one that then takes longer than what
    // is left of that refresh waits a whole refresh more, so a 120 Hz screen shows 60 frames a
    // second. A third lets the next be drawn meanwhile.
    config.desired_maximum_frame_latency = if cfg!(target_os = "ios") { 2 } else { FRAME_LATENCY };
    // a configure the GPU refuses (it couldn't come idle in time, as while an artwork opens) leaves
    // the surface unconfigured: none, so the next frame tries again (else it stayed blank)
    let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
    surface.configure(&gpu.device, &config);
    if let Some(e) = pollster::block_on(scope.pop()) {
        log::warn!("the surface couldn't be configured yet: {e}");
        return None;
    }
    #[cfg(target_os = "ios")]
    crate::window::commit();
    log::info!(
        "surface {}×{} {:?} {:?} {:?}",
        config.width,
        config.height,
        config.format,
        config.present_mode,
        config.color_space
    );
    Some(config)
}

/// Half floats in extended sRGB among `caps`. Extended sRGB only: wgpu's extended Display P3 looks
/// its colour space up in a way that crashes on the iPad simulator. A Display P3 canvas's colours
/// past sRGB go out as values below 0 and past 1, which extended sRGB carries (see `display.wgsl`).
pub(crate) fn offers_extended(caps: &wgpu::SurfaceCapabilities) -> bool {
    caps.format_capabilities.iter().any(|f| {
        f.format == wgpu::TextureFormat::Rgba16Float && f.color_spaces.contains(wgpu::SurfaceColorSpaces::EXTENDED_SRGB)
    })
}

/// [`configure`] for an HDR canvas: half floats in the screen's extended range (extended sRGB, which
/// also carries Display P3's colours), sRGB-encoded as the canvas draws, with light past white
/// shown brighter than it. None where the surface has no extended range.
pub(crate) fn configure_extended(
    gpu: &Gpu,
    surface: &wgpu::Surface<'_>,
    caps: &wgpu::SurfaceCapabilities,
    size: (u32, u32),
) -> Option<wgpu::SurfaceConfiguration> {
    let format = wgpu::TextureFormat::Rgba16Float;
    if !offers_extended(caps) {
        return None;
    }
    let color_space = wgpu::SurfaceColorSpace::ExtendedSrgb;
    let mut config = surface.get_default_config(&gpu.adapter, size.0.max(1), size.1.max(1))?;
    config.format = format;
    config.color_space = color_space;
    config.view_formats = Vec::new();
    config.present_mode = if caps.present_modes.contains(&wgpu::PresentMode::Mailbox) {
        wgpu::PresentMode::Mailbox
    } else {
        wgpu::PresentMode::Fifo
    };
    if caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::Opaque) {
        config.alpha_mode = wgpu::CompositeAlphaMode::Opaque;
    }
    // (as `configure` has it)
    config.desired_maximum_frame_latency = if cfg!(target_os = "ios") { 2 } else { FRAME_LATENCY };
    let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
    surface.configure(&gpu.device, &config);
    if let Some(e) = pollster::block_on(scope.pop()) {
        log::warn!("the surface couldn't be configured for extended range: {e}");
        return None;
    }
    #[cfg(target_os = "ios")]
    crate::window::commit();
    log::info!("surface {}×{} {:?} {:?}", config.width, config.height, config.format, config.color_space);
    Some(config)
}

#[cfg(test)]
mod tests {
    use procraft_gpu::wgpu::PresentMode;

    /// The desktop's windows and the tablets' are configured alike: the newest frame shown at each
    /// refresh where the system offers that, one frame in flight.
    #[test]
    fn a_surface_shows_the_newest_frame_where_it_can_with_one_in_flight() {
        assert_eq!(super::present_mode(&[PresentMode::Fifo, PresentMode::Mailbox]), PresentMode::Mailbox);
        assert_eq!(super::present_mode(&[PresentMode::Fifo, PresentMode::Immediate]), PresentMode::Fifo);
        assert_eq!(super::present_mode(&[]), PresentMode::Fifo);
        assert_eq!(super::FRAME_LATENCY, 1);
    }
}
