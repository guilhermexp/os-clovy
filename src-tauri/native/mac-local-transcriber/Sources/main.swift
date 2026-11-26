import ArgmaxCore
import CoreML
import FluidAudio
import Foundation
import WhisperKit
import enum WhisperKit.Constants

// stdout belongs exclusively to the JSON-lines protocol, including library output.
let protocolOutput = FileHandle(fileDescriptor: dup(STDOUT_FILENO), closeOnDealloc: true)
dup2(STDERR_FILENO, STDOUT_FILENO)

enum ModelID: String, Decodable {
    case ultra = "parakeet-ultra"
    case phonon = "phonon-2.5"
    case pro = "fluid-speech-pro"
    case whisper = "whisper-large-v3-turbo-local"
}

struct Request: Decodable {
    let request_id: UInt64
    let op: String
    let model_id: ModelID
    let model_dir: String?
    let audio_path: String?
    let language: String?
}

struct HelperError: LocalizedError {
    let errorDescription: String?
    init(_ message: String) { errorDescription = message }
}

func respond(_ value: [String: Any], to request: Request?) {
    var value = value
    if let request {
        value["request_id"] = request.request_id
        value["model_id"] = request.model_id.rawValue
    }
    if var data = try? JSONSerialization.data(withJSONObject: value, options: [.sortedKeys]) {
        data.append(10)
        protocolOutput.write(data)
    }
}

// The Pro bundle has the v3 window, frame layout and blank id, but ships its
// own preprocessor, encoder and joint. Bind its components explicitly.
@available(macOS 15.0, *)
func loadFluidSpeechPro(at directory: URL) throws -> AsrModels {
    func component(_ name: String, units: MLComputeUnits) throws -> MLModel {
        let config = MLModelConfiguration()
        config.computeUnits = units
        return try MLModel(
            contentsOf: directory.appendingPathComponent("\(name).mlmodelc"),
            configuration: config
        )
    }

    let preprocessor = try component("Preprocessor", units: .cpuOnly)
    let encoder = try component("Encoder", units: .cpuAndNeuralEngine)
    let decoder = try component("Decoder", units: .cpuAndNeuralEngine)
    let joint = try component("JointDecision", units: .cpuAndNeuralEngine)

    func check(
        _ model: MLModel,
        inputs: [String: [Int]],
        outputs: [String: [Int]]
    ) throws {
        func matches(_ features: [String: MLFeatureDescription], _ shapes: [String: [Int]]) -> Bool {
            shapes.allSatisfy { name, shape in
                features[name]?.multiArrayConstraint?.shape.map(\.intValue) == shape
            }
        }
        guard matches(model.modelDescription.inputDescriptionsByName, inputs),
              matches(model.modelDescription.outputDescriptionsByName, outputs) else {
            throw HelperError("Fluid Speech Pro Core ML tensor shapes differ from the pinned export.")
        }
    }

    try check(
        preprocessor, inputs: ["audio_signal": [1, 240000], "audio_length": [1]],
        outputs: ["mel": [1, 128, 1501], "mel_length": [1]]
    )
    try check(
        encoder, inputs: ["mel": [1, 128, 1501], "mel_length": [1]],
        outputs: ["encoder": [1, 1024, 188], "encoder_length": [1]]
    )
    try check(
        decoder, inputs: ["targets": [1, 1], "target_length": [1], "h_in": [2, 1, 640], "c_in": [2, 1, 640]],
        outputs: ["decoder": [1, 640, 1], "h_out": [2, 1, 640], "c_out": [2, 1, 640]]
    )
    try check(
        joint, inputs: ["encoder_step": [1, 1024, 1], "decoder_step": [1, 640, 1]],
        outputs: ["token_id": [1, 1, 1], "token_prob": [1, 1, 1], "duration": [1, 1, 1]]
    )

    let vocabPath = directory.appendingPathComponent("parakeet_v3_vocab.json")
    let stored = try JSONDecoder().decode([Int: String].self, from: Data(contentsOf: vocabPath))
    guard (0..<8192).allSatisfy({ stored[$0] != nil }) else {
        throw HelperError("Fluid Speech Pro vocabulary does not match the v3 token ids.")
    }
    let config = MLModelConfiguration()
    config.computeUnits = .cpuAndNeuralEngine
    return AsrModels(
        encoder: encoder, preprocessor: preprocessor, decoder: decoder, joint: joint,
        configuration: config, vocabulary: stored, version: .v3
    )
}

// WhisperKit's standard tokenizer lookup falls back to the Hub on a local
// load error even with download:false. This factory only opens local files.
struct OfflineWhisperTokenizer: WhisperTokenizer {
    let tokenizer: TokenizerWrapper
    let specialTokens: SpecialTokens
    let allLanguageTokens: Set<Int>

    init(_ tokenizer: TokenizerWrapper) throws {
        self.tokenizer = tokenizer
        func required(_ token: String) throws -> Int {
            guard let id = tokenizer.convertTokenToId(token) else {
                throw HelperError("The local Whisper tokenizer lacks \(token).")
            }
            return id
        }
        let end = try required("<|endoftext|>")
        guard let space = tokenizer.encode(text: " ").first else {
            throw HelperError("The local Whisper tokenizer cannot encode a space.")
        }
        specialTokens = try SpecialTokens(
            endToken: end,
            englishToken: required("<|en|>"),
            noSpeechToken: required("<|nospeech|>"),
            noTimestampsToken: required("<|notimestamps|>"),
            specialTokenBegin: end,
            startOfPreviousToken: required("<|startofprev|>"),
            startOfTranscriptToken: required("<|startoftranscript|>"),
            timeTokenBegin: required("<|0.00|>"),
            transcribeToken: required("<|transcribe|>"),
            translateToken: required("<|translate|>"),
            whitespaceToken: space
        )
        allLanguageTokens = Set(Constants.languages.values.compactMap {
            tokenizer.convertTokenToId("<|\($0)|>")
        })
    }

    func encode(text: String) -> [Int] { tokenizer.encode(text: text) }
    func decode(tokens: [Int]) -> String { tokenizer.decode(tokens: tokens) }
    func convertTokenToId(_ token: String) -> Int? { tokenizer.convertTokenToId(token) }
    func convertIdToToken(_ id: Int) -> String? { tokenizer.convertIdToToken(id) }

    // The helper uses segment times and disables word alignment. Preserve
    // the full Unicode text should the tokenizer be asked for a fragment.
    func splitToWordTokens(tokenIds: [Int]) -> (words: [String], wordTokens: [[Int]]) {
        tokenIds.isEmpty ? ([], []) : ([decode(tokens: tokenIds)], [tokenIds])
    }
}

final class OfflineWhisper: WhisperKit {
    override func loadTokenizerIfNeeded() async throws {
        if tokenizer != nil { return }
        guard let tokenizerFolder else { throw HelperError("Missing local Whisper tokenizer directory.") }
        let local = try await AutoTokenizerWrapper.from(modelFolder: tokenizerFolder)
        tokenizer = try OfflineWhisperTokenizer(local)
    }
}

enum Engine {
    case tdt(AsrManager)
    case whisper(WhisperKit)

    static func load(_ id: ModelID, from directory: URL) async throws -> Self {
        if id == .whisper {
            let weights = directory.appendingPathComponent("openai_whisper-large-v3-v20240930_turbo")
            let tokenizer = directory.appendingPathComponent("tokenizer")
            let options = WhisperKitConfig(
                modelFolder: weights.path,
                tokenizerFolder: tokenizer,
                verbose: false,
                prewarm: false,
                load: true,
                download: false
            )
            return .whisper(try await OfflineWhisper(options))
        }

        let models: AsrModels
        switch id {
        case .ultra:
            models = try AsrModels.loadLocal(from: directory, version: .ultra)
        case .phonon:
            guard #available(macOS 15.0, *) else { throw HelperError("Phonon-2.5 requires macOS 15.") }
            models = try AsrModels.loadLocal(from: directory, version: .phonon25, encoderComputeUnits: .cpuAndGPU)
        case .pro:
            guard #available(macOS 15.0, *) else { throw HelperError("Fluid Speech Pro requires macOS 15.") }
            models = try loadFluidSpeechPro(at: directory)
        case .whisper:
            throw HelperError("Expected a TDT model.")
        }
        let manager = AsrManager()
        try await manager.loadModels(models)
        return .tdt(manager)
    }

    func transcribe(path: String, language hint: String?) async throws -> [String: Any] {
        switch self {
        case .tdt(let manager):
            var state = try TdtDecoderState()
            let language = hint.flatMap(Language.init(rawValue:))
            let result = try await manager.transcribe(URL(fileURLWithPath: path), decoderState: &state, language: language)
            let duration = Int((result.duration * 1000).rounded())
            let tokens: [[String: Any]] = (result.tokenTimings ?? []).map {
                let start = min(duration, max(0, Int(($0.startTime * 1000).rounded())))
                let end = min(duration, max(start, Int(($0.endTime * 1000).rounded())))
                return ["t": $0.token, "start_ms": start, "end_ms": end]
            }
            return ["text": result.text, "tokens": tokens, "duration_ms": duration, "language": language?.rawValue as Any? ?? NSNull()]
        case .whisper(let kit):
            var options = DecodingOptions(language: hint, skipSpecialTokens: true, wordTimestamps: false)
            options.detectLanguage = hint == nil
            let results = try await kit.transcribe(audioPath: path, decodeOptions: options)
            let segments = results.flatMap(\.segments)
            let tokens: [[String: Any]] = segments.map {
                ["t": $0.text, "start_ms": Int(($0.start * 1000).rounded()), "end_ms": Int(($0.end * 1000).rounded())]
            }
            let duration = Int(((segments.map(\.end).max() ?? 0) * 1000).rounded())
            let text = results.map(\.text).joined(separator: " ").trimmingCharacters(in: .whitespacesAndNewlines)
            return ["text": text, "tokens": tokens, "duration_ms": duration, "language": results.first?.language as Any? ?? NSNull()]
        }
    }
}

nonisolated func runServer() async {
    var loaded: (id: ModelID, directory: String, engine: Engine)?
    while let line = readLine() {
        var decoded: Request?
        do {
            let request = try JSONDecoder().decode(Request.self, from: Data(line.utf8))
            decoded = request
            switch request.op {
            case "load":
                guard let directory = request.model_dir else { throw HelperError("Expected a model directory.") }
                let reused = loaded?.id == request.model_id && loaded?.directory == directory
                let start = ContinuousClock.now
                if !reused {
                    loaded = nil
                    let engine = try await Engine.load(request.model_id, from: URL(fileURLWithPath: directory))
                    loaded = (request.model_id, directory, engine)
                }
                let elapsed = start.duration(to: .now).components
                respond(["ok": true, "reused": reused, "load_ms": Double(elapsed.seconds) * 1000 + Double(elapsed.attoseconds) / 1e15], to: request)
            case "transcribe":
                guard let loaded, loaded.id == request.model_id, let path = request.audio_path else {
                    throw HelperError("Load the requested model before transcribing.")
                }
                var result = try await loaded.engine.transcribe(path: path, language: request.language)
                result["ok"] = true
                respond(result, to: request)
            case "unload":
                guard loaded?.id == request.model_id else { throw HelperError("The requested model is not loaded.") }
                loaded = nil
                respond(["ok": true], to: request)
            default:
                throw HelperError("Unknown operation.")
            }
        } catch {
            respond(["ok": false, "error": error.localizedDescription], to: decoded)
        }
    }
}

await runServer()
