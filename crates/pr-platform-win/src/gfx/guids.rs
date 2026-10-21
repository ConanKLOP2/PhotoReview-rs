//! Port of `PhotoReview.Shell.Interop/Graphics/Guids.cs` (52 lines, WP-13b), baseline `d92d6aae`. Unit W8-11f.
//!
//! `windows-rs` takes every IID from the interface type (`Interface::IID`), so production code never passes a GUID by hand
//! (C# needed them for `[Guid]`, `QueryInterface`, `GetBuffer`, `D2D1CreateFactory`). This table keeps the 27 IIDs the C# declared,
//! as the audit anchor: a test proves each equals `<I as Interface>::IID` of the matching `windows` type, which is the Rust twin of the
//! C# rule "every IID is exercised by `D2DSmokeTests`: one wrong character turns a test red".

use windows::core::GUID;

pub const DXGI_OBJECT: GUID = GUID::from_u128(0xaec22fb8_76f3_4639_9be0_28eb43a67a2e);
pub const DXGI_DEVICE_SUB_OBJECT: GUID = GUID::from_u128(0x3d3e0379_f9de_4d58_bb6c_18d62992f1a6);
pub const DXGI_ADAPTER: GUID = GUID::from_u128(0x2411e7e1_12ac_4ccf_bd14_9798e8534dc0);
pub const DXGI_DEVICE: GUID = GUID::from_u128(0x54ec77fa_1377_44e6_8c32_88fd5f44c84c);
pub const DXGI_DEVICE1: GUID = GUID::from_u128(0x77db970f_6276_48ba_ba28_070143b4392c);
pub const DXGI_SURFACE: GUID = GUID::from_u128(0xcafcb56c_6ac3_4889_bf47_9e23bbd260ec);
pub const DXGI_FACTORY: GUID = GUID::from_u128(0x7b7166ec_21c7_44ae_b21a_c9ae321ae369);
pub const DXGI_FACTORY1: GUID = GUID::from_u128(0x770aae78_f26f_4dba_a829_253c83d1b387);
pub const DXGI_FACTORY2: GUID = GUID::from_u128(0x50c83a1c_e072_4c48_87b0_3630fa36a6d0);
pub const DXGI_SWAP_CHAIN: GUID = GUID::from_u128(0x310d36a0_d2e7_4c0a_aa04_6a9d23b8886a);
pub const DXGI_SWAP_CHAIN1: GUID = GUID::from_u128(0x790a45f7_0d42_4876_983a_0a55cfe6f4aa);
pub const DXGI_SWAP_CHAIN2: GUID = GUID::from_u128(0xa8be2ac4_199f_4946_b331_79599fb98de7);
pub const D3D11_DEVICE: GUID = GUID::from_u128(0xdb6f6ddb_ac77_4e88_8253_819df9bbf140);
pub const D2D1_FACTORY: GUID = GUID::from_u128(0x06152247_6f50_465a_9245_118bfd3b6007);
pub const D2D1_FACTORY1: GUID = GUID::from_u128(0xbb12d362_daee_4b9a_aa1d_14ba401cfa1f);
pub const D2D1_RESOURCE: GUID = GUID::from_u128(0x2cd90691_12e2_11dc_9fed_001143a055f9);
pub const D2D1_DEVICE: GUID = GUID::from_u128(0x47dd575d_ac05_4cdd_8049_9b02cd16f44c);
pub const D2D1_IMAGE: GUID = GUID::from_u128(0x65019f75_8da2_497c_b32c_dfa34e48ede6);
pub const D2D1_BITMAP: GUID = GUID::from_u128(0xa2296057_ea42_4099_983b_539fb6505426);
pub const D2D1_BITMAP1: GUID = GUID::from_u128(0xa898a84c_3873_4588_b08b_ebbf978df041);
pub const D2D1_BRUSH: GUID = GUID::from_u128(0x2cd906a8_12e2_11dc_9fed_001143a055f9);
pub const D2D1_SOLID_COLOR_BRUSH: GUID = GUID::from_u128(0x2cd906a9_12e2_11dc_9fed_001143a055f9);
pub const D2D1_RENDER_TARGET: GUID = GUID::from_u128(0x2cd90694_12e2_11dc_9fed_001143a055f9);
pub const D2D1_DEVICE_CONTEXT: GUID = GUID::from_u128(0xe8f7fe7a_191c_466d_ad95_975678bda998);
pub const DWRITE_FACTORY: GUID = GUID::from_u128(0xb859ee5a_d838_4b5b_a2e8_1adc7d93db48);
pub const DWRITE_TEXT_FORMAT: GUID = GUID::from_u128(0x9c906818_31d7_4fd3_a151_7c5e225db55a);
pub const DWRITE_TEXT_LAYOUT: GUID = GUID::from_u128(0x53737037_6d14_410b_9bfe_0b182bb70961);

/// Every IID above with its C# member name (`GraphicsGuids.<Name>`), for the audit test.
pub const ALL: [(&str, GUID); 27] = [
    ("DxgiObject", DXGI_OBJECT),
    ("DxgiDeviceSubObject", DXGI_DEVICE_SUB_OBJECT),
    ("DxgiAdapter", DXGI_ADAPTER),
    ("DxgiDevice", DXGI_DEVICE),
    ("DxgiDevice1", DXGI_DEVICE1),
    ("DxgiSurface", DXGI_SURFACE),
    ("DxgiFactory", DXGI_FACTORY),
    ("DxgiFactory1", DXGI_FACTORY1),
    ("DxgiFactory2", DXGI_FACTORY2),
    ("DxgiSwapChain", DXGI_SWAP_CHAIN),
    ("DxgiSwapChain1", DXGI_SWAP_CHAIN1),
    ("DxgiSwapChain2", DXGI_SWAP_CHAIN2),
    ("D3D11Device", D3D11_DEVICE),
    ("D2D1Factory", D2D1_FACTORY),
    ("D2D1Factory1", D2D1_FACTORY1),
    ("D2D1Resource", D2D1_RESOURCE),
    ("D2D1Device", D2D1_DEVICE),
    ("D2D1Image", D2D1_IMAGE),
    ("D2D1Bitmap", D2D1_BITMAP),
    ("D2D1Bitmap1", D2D1_BITMAP1),
    ("D2D1Brush", D2D1_BRUSH),
    ("D2D1SolidColorBrush", D2D1_SOLID_COLOR_BRUSH),
    ("D2D1RenderTarget", D2D1_RENDER_TARGET),
    ("D2D1DeviceContext", D2D1_DEVICE_CONTEXT),
    ("DWriteFactory", DWRITE_FACTORY),
    ("DWriteTextFormat", DWRITE_TEXT_FORMAT),
    ("DWriteTextLayout", DWRITE_TEXT_LAYOUT),
];

#[cfg(test)]
mod tests {
    use windows::Win32::Graphics::Direct2D::{
        ID2D1Bitmap, ID2D1Bitmap1, ID2D1Brush, ID2D1Device, ID2D1DeviceContext, ID2D1Factory, ID2D1Factory1,
        ID2D1Image, ID2D1RenderTarget, ID2D1Resource, ID2D1SolidColorBrush,
    };
    use windows::Win32::Graphics::Direct3D11::ID3D11Device;
    use windows::Win32::Graphics::DirectWrite::{IDWriteFactory, IDWriteTextFormat, IDWriteTextLayout};
    use windows::Win32::Graphics::Dxgi::{
        IDXGIAdapter, IDXGIDevice, IDXGIDevice1, IDXGIDeviceSubObject, IDXGIFactory, IDXGIFactory1, IDXGIFactory2,
        IDXGIObject, IDXGISurface, IDXGISwapChain, IDXGISwapChain1, IDXGISwapChain2,
    };
    use windows::core::{GUID, Interface};

    // C#: D2DSmokeTests (every IID in GraphicsGuids is exercised; a wrong character turns a test red). Written with the contract,
    // so the table is proven; the W8-11f porter keeps it and adds the rest of the test plan.
    #[test]
    fn all_table_equals_the_windows_rs_interface_ids() {
        let expected: [(&str, GUID); 27] = [
            ("DxgiObject", IDXGIObject::IID),
            ("DxgiDeviceSubObject", IDXGIDeviceSubObject::IID),
            ("DxgiAdapter", IDXGIAdapter::IID),
            ("DxgiDevice", IDXGIDevice::IID),
            ("DxgiDevice1", IDXGIDevice1::IID),
            ("DxgiSurface", IDXGISurface::IID),
            ("DxgiFactory", IDXGIFactory::IID),
            ("DxgiFactory1", IDXGIFactory1::IID),
            ("DxgiFactory2", IDXGIFactory2::IID),
            ("DxgiSwapChain", IDXGISwapChain::IID),
            ("DxgiSwapChain1", IDXGISwapChain1::IID),
            ("DxgiSwapChain2", IDXGISwapChain2::IID),
            ("D3D11Device", ID3D11Device::IID),
            ("D2D1Factory", ID2D1Factory::IID),
            ("D2D1Factory1", ID2D1Factory1::IID),
            ("D2D1Resource", ID2D1Resource::IID),
            ("D2D1Device", ID2D1Device::IID),
            ("D2D1Image", ID2D1Image::IID),
            ("D2D1Bitmap", ID2D1Bitmap::IID),
            ("D2D1Bitmap1", ID2D1Bitmap1::IID),
            ("D2D1Brush", ID2D1Brush::IID),
            ("D2D1SolidColorBrush", ID2D1SolidColorBrush::IID),
            ("D2D1RenderTarget", ID2D1RenderTarget::IID),
            ("D2D1DeviceContext", ID2D1DeviceContext::IID),
            ("DWriteFactory", IDWriteFactory::IID),
            ("DWriteTextFormat", IDWriteTextFormat::IID),
            ("DWriteTextLayout", IDWriteTextLayout::IID),
        ];
        for (i, (name, iid)) in expected.iter().enumerate() {
            assert_eq!(super::ALL[i].0, *name);
            assert_eq!(super::ALL[i].1, *iid, "{name}");
        }
    }
}
