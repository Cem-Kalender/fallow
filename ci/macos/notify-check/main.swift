import AppKit
import UserNotifications
let out = FileHandle(forWritingAtPath: CommandLine.arguments[1]) ?? FileHandle.standardError
func log(_ s: String) { out.write((s + "\n").data(using: .utf8)!) }
class D: NSObject, UNUserNotificationCenterDelegate {
  func userNotificationCenter(_ c: UNUserNotificationCenter, willPresent n: UNNotification, withCompletionHandler h: @escaping (UNNotificationPresentationOptions) -> Void) { h([.banner, .list]) }
}
let app = NSApplication.shared
app.setActivationPolicy(.accessory)
let d = D()
let c = UNUserNotificationCenter.current()
c.delegate = d
let opts: UNAuthorizationOptions = CommandLine.arguments.count > 2 && CommandLine.arguments[2] == "provisional" ? [.alert, .sound, .provisional] : [.alert, .sound]
c.getNotificationSettings { s in log("before: authorizationStatus=\(s.authorizationStatus.rawValue)") }
c.requestAuthorization(options: opts) { granted, err in
  log("granted=\(granted) error=\(String(describing: err))")
  c.getNotificationSettings { s in
    log("authorizationStatus=\(s.authorizationStatus.rawValue) alertSetting=\(s.alertSetting.rawValue)")
    let content = UNMutableNotificationContent()
    content.title = "Fallow spike"; content.body = "Ad-hoc signed notification test"
    c.add(UNNotificationRequest(identifier: UUID().uuidString, content: content, trigger: nil)) { e in
      log("add error=\(String(describing: e))")
      DispatchQueue.main.asyncAfter(deadline: .now() + 3) { c.getDeliveredNotifications { ns in log("delivered=\(ns.count)"); exit(0) } }
    }
  }
}
DispatchQueue.main.asyncAfter(deadline: .now() + 120) { log("timeout"); exit(2) }
app.run()
