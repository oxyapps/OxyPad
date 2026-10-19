use std::borrow::Cow;

use gpui_kit::{AssetSource, Result, SharedString};

// Lucide icons used by the toolbar beyond the component library's default
// set. Only these SVGs are embedded.
gpui_kit::assets::icon_assets!(
    pub ToolbarIcons,
    [FilePlus, Save, SavePen, TextWrap, ScrollText, Columns2]
);

/// The component library's icons plus [`ToolbarIcons`].
pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(bytes) = ToolbarIcons.load(path)? {
            return Ok(Some(bytes));
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = gpui_kit::assets::Assets.list(path)?;
        paths.extend(ToolbarIcons.list(path)?);
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}
