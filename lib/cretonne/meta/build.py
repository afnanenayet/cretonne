# Second-level build script.
#
# This script is run from src/libcretonne/build.rs to generate Rust files.

from __future__ import absolute_import
import argparse
import isa
import gen_types
import gen_instr
import gen_settings
import gen_build_deps
import gen_encoding

parser = argparse.ArgumentParser(description='Generate sources for Cretonne.')
parser.add_argument('--out-dir', help='set output directory')

args = parser.parse_args()
out_dir = args.out_dir

isas = isa.all_isas()

gen_types.generate(out_dir)
gen_instr.generate(isas, out_dir)
gen_settings.generate(isas, out_dir)
gen_encoding.generate(isas, out_dir)
gen_build_deps.generate()