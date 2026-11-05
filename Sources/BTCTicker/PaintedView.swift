import AppKit

/// Paints `draw(_:)` on the CPU into the layer's contents. Redrawing a window of chart size through AppKit's own draw
/// path makes macOS hold about 500 MB of graphics memory for a second or two after every redraw. A CPU bitmap costs only its pixels.
class PaintedView: NSView {
    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
    }

    required init?(coder: NSCoder) {
        super.init(coder: coder)
        wantsLayer = true
    }

    override var wantsUpdateLayer: Bool { true }

    override func viewDidChangeBackingProperties() {
        super.viewDidChangeBackingProperties()
        needsDisplay = true
    }

    override func updateLayer() {
        guard let layer else { return }
        let scale = window?.backingScaleFactor ?? 1
        guard let context = CGContext(
            data: nil,
            width: Int((bounds.width * scale).rounded(.up)),
            height: Int((bounds.height * scale).rounded(.up)),
            bitsPerComponent: 8,
            bytesPerRow: 0,
            space: CGColorSpace(name: CGColorSpace.sRGB)!,
            bitmapInfo: CGImageAlphaInfo.premultipliedFirst.rawValue | CGBitmapInfo.byteOrder32Little.rawValue
        ) else {
            layer.contents = nil
            return
        }
        context.scaleBy(x: scale, y: scale)
        if isFlipped {
            context.translateBy(x: 0, y: bounds.height)
            context.scaleBy(x: 1, y: -1)
        }
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = NSGraphicsContext(cgContext: context, flipped: isFlipped)
        draw(bounds)
        NSGraphicsContext.restoreGraphicsState()
        layer.contents = context.makeImage()
        layer.contentsScale = scale
    }
}
