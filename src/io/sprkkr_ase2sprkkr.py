# Helper run by CView to write an SPR-KKR potential with ase2sprkkr.
#
# Reads a JSON request on stdin:
#   {"path": "<output>", "lattice": [[..],[..],[..]],   # Angstrom
#    "sites": [{"position": [x, y, z],                   # Angstrom, Cartesian
#               "occupants": [{"element": "Fe", "conc": 0.4}, ...]}, ...]}
# and writes the potential file with ase2sprkkr's own writer, so the format is
# whatever the installed ase2sprkkr produces (and matches the SPR-KKR it drives).
#
# Exit status: 0 written (version printed as JSON), 3 ase/ase2sprkkr not
# importable, anything else a failure (message on stderr).
import json
import sys


def main():
    try:
        from ase import Atoms
        from ase2sprkkr.potentials.potentials import Potential
        from ase2sprkkr.sprkkr.sprkkr_atoms import SPRKKRAtoms
    except ImportError as e:
        sys.stderr.write("cannot import ase2sprkkr: %s\n" % e)
        return 3

    req = json.load(sys.stdin)
    sites = req["sites"]

    # One atom per CView site, as the first occupant; the mixtures follow.
    atoms = Atoms(
        symbols=[s["occupants"][0]["element"] for s in sites],
        positions=[s["position"] for s in sites],
        cell=req["lattice"],
        pbc=True,
    )
    # symmetry=False: every site stays its own site (own types, mesh and
    # reference), exactly as in the file CView read. Merging by symmetry would
    # change NT/NM and is the user's choice in ase2sprkkr, not an export side effect.
    atoms = SPRKKRAtoms.promote_ase_atoms(atoms, symmetry=False)
    for i, s in enumerate(sites):
        if len(s["occupants"]) > 1:
            atoms.sites[i].occupation.set({o["element"]: o["conc"] for o in s["occupants"]})

    Potential.from_atoms(atoms).save_to_file(req["path"])

    try:
        from importlib.metadata import version

        v = version("ase2sprkkr")
    except Exception:
        v = "unknown"
    print(json.dumps({"version": v}))
    return 0


if __name__ == "__main__":
    sys.exit(main())
