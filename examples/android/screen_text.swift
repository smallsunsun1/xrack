import CoreML
import Foundation
import ImageIO
import Vision

let screenshot = FileHandle.standardInput.readDataToEndOfFile()
let fullSize = CommandLine.arguments.dropFirst().contains("--full-size")
let thumbnailOptions =
  [
    kCGImageSourceCreateThumbnailFromImageAlways: true,
    kCGImageSourceThumbnailMaxPixelSize: fullSize ? 2400 : 1200,
  ] as CFDictionary
guard let source = CGImageSourceCreateWithData(screenshot as CFData, nil),
  let image = CGImageSourceCreateThumbnailAtIndex(source, 0, thumbnailOptions)
else {
  throw CocoaError(.fileReadCorruptFile)
}
let request = VNRecognizeTextRequest()
request.recognitionLevel = .fast
request.recognitionLanguages = ["en-US"]
if let argument = CommandLine.arguments.first(where: { $0.hasPrefix("--row-y=") }) {
  guard let row = Double(argument.dropFirst("--row-y=".count)), (0...1).contains(row)
  else {
    throw CocoaError(.fileReadCorruptFile)
  }
  let bottom = max(0, 1 - row - 0.035)
  let top = min(1, 1 - row + 0.035)
  request.regionOfInterest = CGRect(x: 0, y: bottom, width: 1, height: top - bottom)
}
if #available(macOS 14, *) {
  for (stage, devices) in try request.supportedComputeStageDevices {
    if let cpu = devices.first(where: { device in
      if case .cpu = device { return true }
      return false
    }) {
      request.setComputeDevice(cpu, for: stage)
    }
  }
} else {
  request.usesCPUOnly = true
}
try VNImageRequestHandler(cgImage: image).perform([request])
for result in request.results ?? [] {
  if let text = result.topCandidates(1).first?.string {
    let bounds = result.boundingBox
    let region = request.regionOfInterest
    let x = region.minX + bounds.midX * region.width
    let y = 1 - (region.minY + bounds.midY * region.height)
    print("\(x)\t\(y)\t\(text)")
  }
}
