import Foundation
import Security

/// API key in the macOS Keychain; other settings in UserDefaults.
enum KeyStore {
    private static let service = "app.agent-groovebox.agentllm"
    private static let account = "api-key"

    static func load() -> String? {
        let q: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne,
        ]
        var out: CFTypeRef?
        guard SecItemCopyMatching(q as CFDictionary, &out) == errSecSuccess,
              let d = out as? Data, let s = String(data: d, encoding: .utf8) else { return nil }
        let k = s.trimmingCharacters(in: .whitespacesAndNewlines)
        return k.isEmpty ? nil : k
    }

    @discardableResult
    static func save(_ key: String) -> Bool {
        let k = key.trimmingCharacters(in: .whitespacesAndNewlines)
        let base: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
        ]
        SecItemDelete(base as CFDictionary)
        if k.isEmpty { return true }
        var add = base
        add[kSecValueData as String] = Data(k.utf8)
        return SecItemAdd(add as CFDictionary, nil) == errSecSuccess
    }

    /// Read a key file (first line, trimmed).
    static func readKeyFile(_ url: URL) -> String? {
        guard let s = try? String(contentsOf: url, encoding: .utf8) else { return nil }
        let k = s.trimmingCharacters(in: .whitespacesAndNewlines)
        return k.isEmpty ? nil : k
    }

    /// Default location used on this machine for the agentllm key.
    static var defaultKeyFile: URL {
        FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Dev/AI-passport/private/agentllm.key")
    }

    /// The key to use right now: the Keychain entry, or the default key file when the
    /// Keychain cannot be read (each rebuild changes the ad-hoc signature, and macOS
    /// may then refuse access until the user allows it again).
    static func current() -> String? {
        load() ?? readKeyFile(defaultKeyFile)
    }

    /// On first launch, import the key from the default file if the Keychain is empty.
    static func bootstrap() {
        if load() == nil, let k = readKeyFile(defaultKeyFile) {
            save(k)
        }
    }
}

enum AppSettings {
    static let defaultModels = "deepseek-v4.1-flash,c-music-express,gemini-3-flash-preview,gpt-5-mini"
    static let defaultBaseURL = "https://agentllm.linkyun.co"

    static var baseURL: String {
        get { UserDefaults.standard.string(forKey: "baseURL") ?? defaultBaseURL }
        set { UserDefaults.standard.set(newValue, forKey: "baseURL") }
    }
    static var models: String {
        get { UserDefaults.standard.string(forKey: "models") ?? defaultModels }
        set { UserDefaults.standard.set(newValue, forKey: "models") }
    }

    /// Where performances are stored.
    static var libraryDir: URL {
        let d = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Music/Agent Groovebox")
        try? FileManager.default.createDirectory(at: d, withIntermediateDirectories: true)
        return d
    }

    /// The bundled `agb` engine binary (falls back to the repo build for `swift run`).
    static var agbBinary: URL? {
        if let r = Bundle.main.resourceURL?.appendingPathComponent("agb"),
           FileManager.default.isExecutableFile(atPath: r.path) { return r }
        if let env = ProcessInfo.processInfo.environment["AGB_BIN"],
           FileManager.default.isExecutableFile(atPath: env) { return URL(fileURLWithPath: env) }
        // app/Sources/AgentGroovebox/Settings.swift → repo/target/release/agb
        let repo = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent().deletingLastPathComponent()
            .deletingLastPathComponent().deletingLastPathComponent()
        let dev = repo.appendingPathComponent("target/release/agb")
        return FileManager.default.isExecutableFile(atPath: dev.path) ? dev : nil
    }

    /// Fetch available model ids from the gateway.
    static func fetchModels(key: String) async throws -> [String] {
        var base = baseURL.trimmingCharacters(in: .whitespaces)
        while base.hasSuffix("/") { base.removeLast() }
        let url = URL(string: base.hasSuffix("/v1") ? "\(base)/models" : "\(base)/v1/models")!
        var req = URLRequest(url: url, timeoutInterval: 20)
        req.setValue("Bearer \(key)", forHTTPHeaderField: "Authorization")
        let (data, resp) = try await URLSession.shared.data(for: req)
        guard (resp as? HTTPURLResponse)?.statusCode == 200 else {
            throw NSError(domain: "models", code: (resp as? HTTPURLResponse)?.statusCode ?? -1)
        }
        let obj = try JSONSerialization.jsonObject(with: data) as? [String: Any]
        let ids = (obj?["data"] as? [[String: Any]])?.compactMap { $0["id"] as? String } ?? []
        return ids.filter { $0 != "*" }.sorted()
    }
}
