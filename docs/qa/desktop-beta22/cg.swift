import Foundation
import CoreGraphics
let a = CommandLine.arguments
func post(_ t: CGEventType, _ p: CGPoint) {
  CGEvent(mouseEventSource: nil, mouseType: t, mouseCursorPosition: p, mouseButton: .left)?.post(tap: .cghidEventTap)
}
switch a[1] {
case "win":
  let pid = Int(a[2])!
  let list = CGWindowListCopyWindowInfo([.optionAll], kCGNullWindowID) as! [[String: Any]]
  for w in list where (w[kCGWindowOwnerPID as String] as? Int) == pid {
    let b = w[kCGWindowBounds as String] as! [String: Any]
    print(w[kCGWindowNumber as String]!, w[kCGWindowLayer as String]!, w[kCGWindowIsOnscreen as String] ?? 0, b["X"]!, b["Y"]!, b["Width"]!, b["Height"]!, w[kCGWindowName as String] ?? "")
  }
case "click":
  let p = CGPoint(x: Double(a[2])!, y: Double(a[3])!)
  post(.mouseMoved, p); usleep(50000)
  post(.leftMouseDown, p); usleep(50000); post(.leftMouseUp, p)
case "cmdclick":
  let p = CGPoint(x: Double(a[2])!, y: Double(a[3])!)
  post(.mouseMoved, p); usleep(50000)
  for t in [CGEventType.leftMouseDown, .leftMouseUp] {
    let e = CGEvent(mouseEventSource: nil, mouseType: t, mouseCursorPosition: p, mouseButton: .left)
    e?.flags = .maskCommand; e?.post(tap: .cghidEventTap); usleep(50000)
  }
case "move":
  post(.mouseMoved, CGPoint(x: Double(a[2])!, y: Double(a[3])!))
case "scroll":
  let p = CGPoint(x: Double(a[2])!, y: Double(a[3])!)
  post(.mouseMoved, p); usleep(50000)
  CGEvent(scrollWheelEvent2Source: nil, units: .pixel, wheelCount: 1, wheel1: Int32(a[4])!, wheel2: 0, wheel3: 0)?.post(tap: .cghidEventTap)
case "front":
  let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as! [[String: Any]]
  for w in list where (w[kCGWindowLayer as String] as? Int) == 0 && (w[kCGWindowName as String] as? String) == "Contour" {
    print(w[kCGWindowNumber as String]!)
  }
default: print("?")
}
