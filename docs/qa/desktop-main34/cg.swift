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
case "key":
  // key <keycode> down|up|press [shift|cmd]
  let code = CGKeyCode(Int(a[2])!)
  var fl: CGEventFlags = []
  if a.count > 4 && a[4] == "shift" { fl = .maskShift }
  if a.count > 4 && a[4] == "cmd" { fl = .maskCommand }
  if a.count > 4 && a[4] == "ctrl" { fl = .maskControl }
  var mk: CGKeyCode? = nil
  if fl == .maskCommand { mk = 55 } else if fl == .maskControl { mk = 59 } else if fl == .maskShift { mk = 56 }
  func m(_ d: Bool) { if let mk = mk { let e = CGEvent(keyboardEventSource: nil, virtualKey: mk, keyDown: d); e?.flags = d ? fl : []; e?.post(tap: .cghidEventTap); usleep(20000) } }
  func k(_ d: Bool) { let e = CGEvent(keyboardEventSource: nil, virtualKey: code, keyDown: d); e?.flags = fl; e?.post(tap: .cghidEventTap) }
  if a[3] == "down" || a[3] == "press" { m(true); k(true) }
  if a[3] == "press" { usleep(80000) }
  if a[3] == "up" || a[3] == "press" { k(false); m(false) }
case "hold":
  // hold <keycode> <repeats>: keydown, autorepeat keydowns, keyup
  let code = CGKeyCode(Int(a[2])!)
  let n = Int(a[3])!
  CGEvent(keyboardEventSource: nil, virtualKey: code, keyDown: true)?.post(tap: .cghidEventTap)
  for _ in 0..<n { usleep(60000); let e = CGEvent(keyboardEventSource: nil, virtualKey: code, keyDown: true); e?.setIntegerValueField(.keyboardEventAutorepeat, value: 1); e?.post(tap: .cghidEventTap) }
  usleep(60000)
  CGEvent(keyboardEventSource: nil, virtualKey: code, keyDown: false)?.post(tap: .cghidEventTap)
case "drag":
  let p1 = CGPoint(x: Double(a[2])!, y: Double(a[3])!), p2 = CGPoint(x: Double(a[4])!, y: Double(a[5])!)
  post(.mouseMoved, p1); usleep(80000)
  post(.leftMouseDown, p1); usleep(150000)
  for i in 1...20 { let t = Double(i)/20; post(.leftMouseDragged, CGPoint(x: p1.x+(p2.x-p1.x)*t, y: p1.y+(p2.y-p1.y)*t)); usleep(30000) }
  usleep(150000); post(.leftMouseUp, p2)
case "type":
  // type <text>: each character as a unicode key event (keycode 0)
  for ch in a[2].utf16 {
    var c = ch
    for d in [true, false] { let e = CGEvent(keyboardEventSource: nil, virtualKey: 0, keyDown: d); e?.keyboardSetUnicodeString(stringLength: 1, unicodeString: &c); e?.post(tap: .cghidEventTap); usleep(30000) }
  }
case "loc":
  let l = CGEvent(source: nil)!.location; print(Int(l.x), Int(l.y))
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
// Never leave a modifier held for the whole machine.
let clr = CGEvent(source: nil)!; clr.type = .flagsChanged; clr.flags = []; clr.post(tap: .cghidEventTap)
print("flags", CGEventSource.flagsState(.hidSystemState).rawValue & 0x00ff0000)
