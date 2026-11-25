// swift-tools-version: 6.1
import PackageDescription

#if compiler(>=6.2)
let fluidAudio: Package.Dependency = .package(url: "https://github.com/FluidInference/FluidAudio", exact: "0.18.3", traits: [])
#else
let fluidAudio: Package.Dependency = .package(url: "https://github.com/FluidInference/FluidAudio", exact: "0.18.3")
#endif

let package = Package(
    name: "ClovyLocalTranscriber",
    platforms: [.macOS(.v14)],
    products: [.executable(name: "clovy-local-transcriber", targets: ["ClovyLocalTranscriber"])],
    dependencies: [fluidAudio, .package(url: "https://github.com/argmaxinc/argmax-oss-swift", exact: "1.1.1")],
    targets: [.executableTarget(name: "ClovyLocalTranscriber", dependencies: [.product(name: "FluidAudio", package: "FluidAudio"), .product(name: "WhisperKit", package: "argmax-oss-swift")], path: "Sources")]
)
