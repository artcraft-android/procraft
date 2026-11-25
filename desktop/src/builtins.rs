//! Whether a way in to one of the app's own features shows, as its built-in plugin stands (see
//! `docs/design/plugins-everything.md`). The panels are handed the features hidden each frame, as
//! they are Lasso Fill's switch, and ask here about a QuickMenu action, an adjustment, a layer's
//! option or a page of Gesture controls; the core says which feature each belongs to.

use procraft_core::{Adjustment, Feature, GesturePurpose, LayerOption, QuickAction};

fn shows(hidden: &[Feature], feature: Option<Feature>) -> bool {
    feature.is_none_or(|f| !hidden.contains(&f))
}

/// Whether QuickMenu action `a` shows.
pub fn action(hidden: &[Feature], a: QuickAction) -> bool {
    shows(hidden, procraft_core::quick_action_feature(a))
}

/// Whether the Adjustments menu lists `a`.
pub fn adjustment(hidden: &[Feature], a: Adjustment) -> bool {
    shows(hidden, procraft_core::adjustment_feature(a))
}

/// Whether a layer's options list `option`.
pub fn layer_option(hidden: &[Feature], option: LayerOption) -> bool {
    shows(hidden, procraft_core::layer_option_feature(option))
}

/// Whether Gesture controls has a page for `purpose`.
pub fn gesture_page(hidden: &[Feature], purpose: GesturePurpose) -> bool {
    shows(hidden, procraft_core::gesture_purpose_feature(purpose))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hidden_feature_hides_its_ways_in_alone() {
        let hidden = [Feature::Liquify, Feature::QuickShape];
        assert!(!adjustment(&hidden, Adjustment::Liquify) && adjustment(&hidden, Adjustment::Curves));
        assert!(!action(&hidden, QuickAction::Liquify) && !action(&hidden, QuickAction::QuickshapeOn));
        assert!(action(&hidden, QuickAction::Undo) && action(&[], QuickAction::Liquify));
        assert!(!gesture_page(&hidden, GesturePurpose::Quickshape) && gesture_page(&hidden, GesturePurpose::Smudge));
        assert!(layer_option(&hidden, LayerOption::ClippingMask));
        // Layer Style and an adjustment layer's settings go with the Effects plugin's switches
        let effects = [Feature::LayerStyles, Feature::AdjustmentLayers];
        assert!(!layer_option(&effects, LayerOption::LayerStyle));
        assert!(!layer_option(&effects, LayerOption::AdjustmentSettings));
        assert!(layer_option(&hidden, LayerOption::LayerStyle));
    }
}
