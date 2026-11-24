// 016 (research M-10, T002): draws the menu bar icon's template images from the logo's outline
// (the page's header logo: three strokes on a 24-unit grid), black on transparent, at 18 pt:
// menubar.png and menubar@2x.png, and the same with a dot at the top right for waiting items
// (menubar-dot.png, menubar-dot@2x.png). macOS colours a template image itself.
//
//   swiftc -O assets/icon/make_menubar.swift -o /tmp/make_menubar && /tmp/make_menubar assets/icon
import AppKit

func draw(scale: CGFloat, dot: Bool, to path: String) {
    let px = Int(18 * scale)
    let space = CGColorSpaceCreateDeviceRGB()
    guard let ctx = CGContext(data: nil, width: px, height: px, bitsPerComponent: 8, bytesPerRow: 0,
                              space: space, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else {
        fatalError("no context")
    }
    // The logo's grid is 24 units with y down; the image is 18 pt with y up.
    let k = CGFloat(px) / 24
    ctx.translateBy(x: 0, y: CGFloat(px))
    ctx.scaleBy(x: k, y: -k)
    ctx.setStrokeColor(CGColor(red: 0, green: 0, blue: 0, alpha: 1))
    ctx.setLineWidth(2)
    ctx.setLineCap(.round)
    ctx.setLineJoin(.round)
    // M4 20c4-1 6-5 8-9s4-7 8-7
    ctx.move(to: CGPoint(x: 4, y: 20))
    ctx.addCurve(to: CGPoint(x: 12, y: 11), control1: CGPoint(x: 8, y: 19), control2: CGPoint(x: 10, y: 15))
    ctx.addCurve(to: CGPoint(x: 20, y: 4), control1: CGPoint(x: 14, y: 7), control2: CGPoint(x: 16, y: 4))
    // M4 14c3 0 5-2 6-4
    ctx.move(to: CGPoint(x: 4, y: 14))
    ctx.addCurve(to: CGPoint(x: 10, y: 10), control1: CGPoint(x: 7, y: 14), control2: CGPoint(x: 9, y: 12))
    // M12 20c3-1 5-3 6-6
    ctx.move(to: CGPoint(x: 12, y: 20))
    ctx.addCurve(to: CGPoint(x: 18, y: 14), control1: CGPoint(x: 15, y: 19), control2: CGPoint(x: 17, y: 17))
    ctx.strokePath()
    if dot {
        // A clear ring around the dot keeps it apart from the stroke under it.
        ctx.setBlendMode(.clear)
        ctx.fillEllipse(in: CGRect(x: 14.5, y: 0.5, width: 9, height: 9))
        ctx.setBlendMode(.normal)
        ctx.setFillColor(CGColor(red: 0, green: 0, blue: 0, alpha: 1))
        ctx.fillEllipse(in: CGRect(x: 16, y: 2, width: 6, height: 6))
    }
    guard let img = ctx.makeImage() else { fatalError("no image") }
    let rep = NSBitmapImageRep(cgImage: img)
    rep.size = NSSize(width: 18, height: 18)
    guard let png = rep.representation(using: .png, properties: [:]) else { fatalError("no png") }
    do { try png.write(to: URL(fileURLWithPath: path)) } catch { fatalError("\(path): \(error)") }
}

let out = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "."
for (dot, name) in [(false, "menubar"), (true, "menubar-dot")] {
    draw(scale: 1, dot: dot, to: "\(out)/\(name).png")
    draw(scale: 2, dot: dot, to: "\(out)/\(name)@2x.png")
}
