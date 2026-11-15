import Foundation
import CoreGraphics
let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as! [[String: Any]]
for w in list {
  let b = w[kCGWindowBounds as String] as! [String: Any]
  let owner = w[kCGWindowOwnerName as String] as? String ?? ""
  if (b["Width"] as! Double) < 100 { continue }
  print(w[kCGWindowNumber as String]!, w[kCGWindowOwnerPID as String]!, owner, w[kCGWindowLayer as String]!, b["X"]!, b["Y"]!, b["Width"]!, b["Height"]!, w[kCGWindowName as String] ?? "")
}
