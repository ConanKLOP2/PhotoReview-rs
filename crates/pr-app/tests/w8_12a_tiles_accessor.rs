//! Kills the mutant that replaces TiledGpuImage::tiles with an empty slice.
use pr_app::services::render::tiled::{GpuTile, TiledGpuImage};
use pr_app::viewport::render_geometry::{PixelRect, TileSpec};
use pr_imaging::pixels::{PixelBuffer, PixelLayout};

#[test]
fn tiles_accessor_returns_the_constructor_tiles_in_order() {
    let px = PixelBuffer::allocate(190, 50, PixelLayout::Bgr32).unwrap();
    let a = TileSpec {
        texture: PixelRect::new(0, 0, 100, 50),
        core: PixelRect::new(0, 0, 90, 50),
    };
    let b = TileSpec {
        texture: PixelRect::new(90, 0, 60, 50),
        core: PixelRect::new(90, 0, 60, 50),
    };
    let image = TiledGpuImage::<u32>::new(&px, vec![GpuTile::new(a, Some(1)), GpuTile::new(b, None)], 7);
    assert_eq!(image.tiles().len(), 2);
    assert_eq!(image.tiles()[0].spec(), a);
    assert_eq!(image.tiles()[1].spec(), b);
    assert!(image.tiles()[0].has_bitmap());
    assert!(!image.tiles()[1].has_bitmap());
    assert_eq!(image.layout(), PixelLayout::Bgr32);
    assert_eq!(image.generation(), 7);
}
