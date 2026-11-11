### Microsoft Visual C++ Redistributable (Windows installer only)

The Windows installer of Pitwall carries Microsoft's Visual C++ Redistributable package, `vc_redist.x64.exe`, version
14.52.37029.0, unmodified, and runs it when the PC lacks that runtime or has an older one. The llama.cpp engine Pitwall
downloads needs it (vcruntime140.dll, msvcp140.dll). It is a Microsoft product under its own licence terms, not part of
Pitwall's own code, and it is not part of the macOS or Linux builds.

Basis for shipping it: the "Distributable Code Files" list (REDIST.TXT) of the Microsoft Software License Terms for Visual
Studio 2026 and later (https://learn.microsoft.com/en-us/visualstudio/releases/2026/redistribution, read 2026-11-11; the Visual
Studio 2022 list has the same wording). Under "Visual C++ Runtime Files" it says: "Subject to the License Terms for the software,
you may copy and distribute with your program any of the files within the following folder and its subfolders except as noted
below. You may not modify these files." and names [VisualStudioFolder]\VC\redist; the Redistributable package is one of the
files there, shipped unmodified. It also says the same package obtained from Microsoft's downloads is "limited to licensed Visual
Studio users". Pitwall's publisher holds a Visual Studio licence (Community edition, which is free for individuals). The package's
own licence terms are shown by its installer. The pinned file, its source address and its SHA-256 are in
apps/pitwall/app/src-tauri/windows/vc-redist.lock.json.
