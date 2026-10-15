# DirectML redistributable notices

The pinned Windows ORT distribution includes DirectML.dll 1.15.4. Its SHA-256 is
`9c9e6d822561c6c41b90e6994b3e8857cf1d66dbfb1e0c4c799c7c89b4e92da1`.
It is byte-identical to bin/x64-win/DirectML.dll in Microsoft's official NuGet
Microsoft.AI.DirectML 1.15.4 package:
https://www.nuget.org/packages/Microsoft.AI.DirectML/1.15.4

NuGet package SHA-256:
`4e7cb7ddce8cf837a7a75dc029209b520ca0101470fcdf275c1f49736a3615b9`.
LICENSE.txt, LICENSE-CODE.txt and ThirdPartyNotices.txt are copied unmodified
from that package. These terms are distinct from ONNX Runtime's MIT license.
The redistributable terms permit distribution with Windows applications;
these notices and the DLL are included only in the Windows archive.

The packager checks the DLL hash so a different redistributable cannot silently
ship with stale notices. Update the provenance, notices and smoke validation
when the pinned ORT distribution changes. No Windows DLL execution has been
validated by this static inspection on Linux.
