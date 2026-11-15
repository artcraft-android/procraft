import AVFoundation
import CoreGraphics
import Foundation
import ImageIO
import UIKit
import UniformTypeIdentifiers
import VideoToolbox

/// The most stops above white the light tool's HDR limit goes to.
let hdrLimitMost: Float = 5

/// The HDR limit as its slider shows it.
func hdrLimitShown(_ stops: Float) -> String {
    let rounded = (stops * 10).rounded() / 10
    return rounded == 0 ? "White" : "+\(presetNumber(v: rounded)) stops"
}

/// HDR artworks on the iPad: the light tool's limit above white, the screen's headroom told to the
/// core as it changes (an HDR canvas's light above white is rolled off to it), and the HDR exports.
extension CanvasModel {
    private static let lightLimitKey = "lightLimitStops"

    /// Whether the artwork is an HDR one (chosen as it was made).
    var isHdr: Bool { session.isHdr() }

    /// As the artwork opens: the light limit last chosen, and (an HDR one's) the screen's headroom
    /// followed while it is open.
    func startHdr() {
        let saved = UserDefaults.standard.object(forKey: Self.lightLimitKey) as? Float
        lightLimit = min(max(saved ?? 2, 0), hdrLimitMost)
        session.setLightLimit(stops: lightLimit)
        guard isHdr else { return }
        // the headroom moves with the screen's brightness and the light around it
        let session = session
        let follow = { session.setDisplayHeadroom(headroom: Float(Self.headroom())) }
        follow()
        Timer.scheduledTimer(withTimeInterval: 0.5, repeats: true) { [weak self] timer in
            guard self != nil else {
                timer.invalidate()
                return
            }
            follow()
        }
    }

    /// The screen's headroom now: how many times white's light it shows at most (1 without
    /// extended range).
    private static func headroom() -> CGFloat {
        let scenes = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }
        let screen = scenes.first { $0.activationState == .foregroundActive }?.screen ?? scenes.first?.screen
        return max(screen?.currentEDRHeadroom ?? 1, 1)
    }

    /// The paint colour lit `stops` above white (an HDR artwork's; 0, as chosen).
    func setColorStops(_ stops: Float) {
        colorStops = min(max(stops, 0), hdrLimitMost)
        session.setColorStops(stops: colorStops)
    }

    func setLightLimit(_ stops: Float) {
        lightLimit = min(max(stops, 0), hdrLimitMost)
        session.setLightLimit(stops: lightLimit)
        UserDefaults.standard.set(lightLimit, forKey: Self.lightLimitKey)
    }

    /// The artwork as an HDR image to share: a 10-bit HEIC (`exr` false) whose light above white
    /// shows in Photos and on HDR screens, or an OpenEXR file for compositing.
    func shareHdr(exr: Bool) {
        exporting(failed: "The HDR image couldn't be made.") { session in
            let base = exportFileName(name: session.name(), package: false)
            let out = Sharing.exportsDir.appendingPathComponent(base + (exr ? ".exr" : ".heic"))
            if exr {
                return (try? session.exportExr(path: out.path)) != nil ? [out] : []
            }
            guard let picture = try? session.hdrPicture() else { return [] }
            guard let data = HDRImage.heic(picture), (try? data.write(to: out, options: .atomic)) != nil else {
                return []
            }
            return [out]
        }
    }
}

/// HDR stills from an HDR artwork's picture. The artwork's white (1.0) is put at the BT.2408
/// graphics white, 203 nits, so its light above white sits where other HDR pictures put theirs.
enum HDRImage {
    /// The picture as a CGImage in its own extended-range colour space (sRGB-encoded, carried on
    /// past white), half floats, premultiplied.
    static func image(_ p: HdrPicture) -> CGImage? {
        let name = p.p3 ? CGColorSpace.extendedDisplayP3 : CGColorSpace.extendedSRGB
        guard let space = CGColorSpace(name: name),
              let provider = CGDataProvider(data: p.half as CFData) else { return nil }
        let info = CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue)
            .union(.byteOrder16Little).union(.floatComponents)
        return CGImage(
            width: Int(p.width), height: Int(p.height), bitsPerComponent: 16, bitsPerPixel: 64,
            bytesPerRow: Int(p.width) * 8, space: space, bitmapInfo: info, provider: provider,
            decode: nil, shouldInterpolate: false, intent: .defaultIntent
        )
    }

    /// A 10-bit HEIC in BT.2100 PQ: what Photos and browsers show as HDR. Light past PQ's peak
    /// clips. As Koi Studio writes its HDR stills.
    static func heic(_ p: HdrPicture, quality: CGFloat = 0.92) -> Data? {
        let (w, h) = (Int(p.width), Int(p.height))
        guard let source = image(p),
              let pq = CGColorSpace(name: CGColorSpace.itur_2100_PQ),
              let context = CGContext(
                  data: nil, width: w, height: h, bitsPerComponent: 16, bytesPerRow: w * 8, space: pq,
                  bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue | CGBitmapInfo.byteOrder16Little.rawValue
              ) else { return nil }
        context.interpolationQuality = .none
        context.setBlendMode(.copy)
        context.draw(source, in: CGRect(x: 0, y: 0, width: w, height: h))
        guard let encoded = context.makeImage() else { return nil }
        let output = NSMutableData()
        guard let destination = CGImageDestinationCreateWithData(output, UTType.heic.identifier as CFString, 1, nil) else {
            return nil
        }
        CGImageDestinationAddImage(destination, encoded, [kCGImageDestinationLossyCompressionQuality: quality] as CFDictionary)
        guard CGImageDestinationFinalize(destination) else { return nil }
        return output as Data
    }
}

extension VideoWriter {
    /// `count` frames `width` x `height` (even) as a 10-bit HEVC video in HLG at `url`, as Koi
    /// Studio writes HDR video: `frame` gives frame n's pixels as the core makes them (half floats,
    /// BT.2020, HLG-encoded; see `animationFrameHlg`). It plays as HDR on screens that have it and
    /// as a picture near the artwork's own on those that don't. False if it wasn't made; nothing is
    /// left at `url` then.
    static func writeHlg(
        to url: URL, width: Int, height: Int, count: Int, fps: Float, frame: (Int) -> Data?
    ) -> Bool {
        let rate = frameRate(fps: fps)
        try? FileManager.default.removeItem(at: url)
        guard let writer = try? AVAssetWriter(outputURL: url, fileType: .mp4) else { return false }
        let settings: [String: Any] = [
            AVVideoCodecKey: AVVideoCodecType.hevc,
            AVVideoWidthKey: width,
            AVVideoHeightKey: height,
            AVVideoCompressionPropertiesKey: [
                AVVideoProfileLevelKey: kVTProfileLevel_HEVC_Main10_AutoLevel as String,
                AVVideoAverageBitRateKey: max(width * height * 6, 2_000_000),
            ],
            AVVideoColorPropertiesKey: [
                AVVideoColorPrimariesKey: AVVideoColorPrimaries_ITU_R_2020,
                AVVideoTransferFunctionKey: AVVideoTransferFunction_ITU_R_2100_HLG,
                AVVideoYCbCrMatrixKey: AVVideoYCbCrMatrix_ITU_R_2020,
            ],
        ]
        let input = AVAssetWriterInput(mediaType: .video, outputSettings: settings)
        input.expectsMediaDataInRealTime = false
        let adaptor = AVAssetWriterInputPixelBufferAdaptor(assetWriterInput: input, sourcePixelBufferAttributes: [
            kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_64RGBAHalf,
            kCVPixelBufferWidthKey as String: width,
            kCVPixelBufferHeightKey as String: height,
        ])
        guard writer.canAdd(input) else { return false }
        writer.add(input)
        guard writer.startWriting() else { return false }
        writer.startSession(atSourceTime: .zero)
        var whole = true
        for n in 0 ..< count {
            while !input.isReadyForMoreMediaData, writer.status == .writing {
                Thread.sleep(forTimeInterval: 0.002)
            }
            var made: CVPixelBuffer?
            guard writer.status == .writing, let pool = adaptor.pixelBufferPool,
                  CVPixelBufferPoolCreatePixelBuffer(nil, pool, &made) == kCVReturnSuccess, let buffer = made,
                  let pixels = frame(n), pixels.count == width * height * 8
            else {
                whole = false
                break
            }
            CVBufferSetAttachment(buffer, kCVImageBufferColorPrimariesKey, kCVImageBufferColorPrimaries_ITU_R_2020, .shouldPropagate)
            CVBufferSetAttachment(
                buffer, kCVImageBufferTransferFunctionKey, kCVImageBufferTransferFunction_ITU_R_2100_HLG, .shouldPropagate
            )
            CVBufferSetAttachment(buffer, kCVImageBufferYCbCrMatrixKey, kCVImageBufferYCbCrMatrix_ITU_R_2020, .shouldPropagate)
            CVPixelBufferLockBaseAddress(buffer, [])
            if let base = CVPixelBufferGetBaseAddress(buffer) {
                let stride = CVPixelBufferGetBytesPerRow(buffer)
                pixels.withUnsafeBytes { source in
                    guard let from = source.baseAddress else { return }
                    for y in 0 ..< height {
                        memcpy(base + y * stride, from + y * width * 8, width * 8)
                    }
                }
            }
            CVPixelBufferUnlockBaseAddress(buffer, [])
            let at = CMTime(value: Int64(n) * Int64(rate.seconds), timescale: CMTimeScale(max(rate.frames, 1)))
            guard adaptor.append(buffer, withPresentationTime: at) else {
                whole = false
                break
            }
        }
        input.markAsFinished()
        guard whole else {
            writer.cancelWriting()
            try? FileManager.default.removeItem(at: url)
            return false
        }
        let finished = DispatchSemaphore(value: 0)
        writer.finishWriting { finished.signal() }
        finished.wait()
        return writer.status == .completed
    }
}
