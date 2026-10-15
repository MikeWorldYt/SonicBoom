# ONNX Runtime native notices

LICENSE and ThirdPartyNotices.txt are unmodified from Microsoft ONNX Runtime
v1.28.0, commit da9b5e364c465de65c49d91e696cd6485270757f:
https://github.com/microsoft/onnxruntime/tree/da9b5e364c465de65c49d91e696cd6485270757f

The locked ort-sys 2.0.0-rc.13 build/download/dist.tsv selects the Pyke
`ms@1.28.0` CPU artifacts for our two release targets and verifies their
SHA-256 hashes during download. Those static archives do not carry separate
notice files, so we preserve the corresponding upstream notices here and
include them in every package. Update these notices when changing the native
ORT distribution. Rust wrapper licensing is also included by the packager.
