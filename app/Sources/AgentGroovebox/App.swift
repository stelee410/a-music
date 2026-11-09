import AppKit
import SwiftUI

final class AppDelegate: NSObject, NSApplicationDelegate {
    func applicationDidFinishLaunching(_ notification: Notification) {
        if let out = ProcessInfo.processInfo.environment["AGB_HEADLESS_CHECK"] {
            Task { @MainActor in HeadlessCheck.run(out: out) }
            return
        }
        // Needed when launched via `swift run` (no bundle): become a regular foreground app.
        NSApp.setActivationPolicy(.regular)
        NSApp.activate(ignoringOtherApps: true)
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        ProcessInfo.processInfo.environment["AGB_HEADLESS_CHECK"] == nil
    }
}

@main
struct AgentGrooveboxApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) var delegate

    var body: some Scene {
        WindowGroup("Agent Groovebox") {
            ContentView()
                .frame(minWidth: 1100, minHeight: 720)
        }
        .windowStyle(.hiddenTitleBar)
        .defaultSize(width: 1280, height: 820)
        Settings {
            SettingsView().frame(width: 520)
        }
    }
}
