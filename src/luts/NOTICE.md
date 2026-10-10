# View transform lookup tables

`aces13.lut.zst` and `aces20.lut.zst` are baked by `tools/view_transforms.py bake-aces` from the
ACES configurations built into OpenColorIO 2.6:

- `cg-config-v2.2.0_aces-v1.3_ocio-v2.4`, view "ACES 1.0 - SDR Video"
- `cg-config-v4.0.0_aces-v2.0_ocio-v2.5`, view "ACES 2.0 - SDR 100 nits (Rec.709)"

OpenColorIO and its built-in configurations: Copyright Contributors to the OpenColorIO
Project, BSD-3-Clause license (https://github.com/AcademySoftwareFoundation/OpenColorIO).
The ACES transforms are by the Academy of Motion Picture Arts and Sciences and the ACES
contributors (https://github.com/aces-aswf).

This notice should be shipped with Crate's releases.
