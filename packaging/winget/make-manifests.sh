#!/usr/bin/env bash
# Generate winget manifests for a published CView release.
#   packaging/winget/make-manifests.sh 1.3.1
# Writes manifests/m/MAVENsResearchGroup/CView/<version>/ under this folder,
# laid out as in microsoft/winget-pkgs, ready to copy into a fork of it.
set -euo pipefail

VERSION="${1:?usage: $0 <version, e.g. 1.3.1>}"
ID="MAVENsResearchGroup.CView"
URL="https://github.com/mavensgroup/cview/releases/download/v${VERSION}/cview-installer.exe"
SCHEMA="1.10.0"

SHA=$(curl -fsSL "${URL}.sha256" | awk '{print toupper($1)}')
[ ${#SHA} -eq 64 ] || { echo "could not read checksum from ${URL}.sha256" >&2; exit 1; }

OUT="$(cd "$(dirname "$0")" && pwd)/manifests/m/MAVENsResearchGroup/CView/${VERSION}"
mkdir -p "$OUT"

cat > "$OUT/${ID}.yaml" <<EOF
# yaml-language-server: \$schema=https://aka.ms/winget-manifest.version.${SCHEMA}.schema.json
PackageIdentifier: ${ID}
PackageVersion: ${VERSION}
DefaultLocale: en-US
ManifestType: version
ManifestVersion: ${SCHEMA}
EOF

cat > "$OUT/${ID}.installer.yaml" <<EOF
# yaml-language-server: \$schema=https://aka.ms/winget-manifest.installer.${SCHEMA}.schema.json
PackageIdentifier: ${ID}
PackageVersion: ${VERSION}
InstallerType: nullsoft
Scope: machine
ElevationRequirement: elevatesSelf
UpgradeBehavior: install
AppsAndFeaturesEntries:
- DisplayName: CView
  Publisher: MAVENs Research Group
  ProductCode: CView
Installers:
- Architecture: x64
  InstallerUrl: ${URL}
  InstallerSha256: ${SHA}
ManifestType: installer
ManifestVersion: ${SCHEMA}
EOF

cat > "$OUT/${ID}.locale.en-US.yaml" <<EOF
# yaml-language-server: \$schema=https://aka.ms/winget-manifest.defaultLocale.${SCHEMA}.schema.json
PackageIdentifier: ${ID}
PackageVersion: ${VERSION}
PackageLocale: en-US
Publisher: MAVENs Research Group
PublisherUrl: https://github.com/mavensgroup
PublisherSupportUrl: https://github.com/mavensgroup/cview/issues
Author: Rudra Banerjee
PackageName: CView
PackageUrl: https://mavensgroup.github.io/cview/
License: MIT
LicenseUrl: https://github.com/mavensgroup/cview/blob/main/LICENSE
ShortDescription: Crystal structure visualization and analysis for DFT researchers
Description: |-
  CView is a desktop application for visualizing and analyzing crystal structures,
  aimed at ab-initio DFT workflows (VASP, Quantum ESPRESSO, SPR-KKR). It reads and
  writes common crystallographic formats and provides symmetry analysis, k-paths,
  XRD simulation, supercell and slab construction, bond-valence analysis and
  charge-density visualization.
Moniker: cview
Tags:
- crystallography
- dft
- materials-science
- visualization
- xrd
- vasp
- quantum-espresso
ReleaseNotesUrl: https://github.com/mavensgroup/cview/releases/tag/v${VERSION}
Documentations:
- DocumentLabel: User guide
  DocumentUrl: https://mavensgroup.github.io/cview/
ManifestType: defaultLocale
ManifestVersion: ${SCHEMA}
EOF

echo "Wrote $OUT"
