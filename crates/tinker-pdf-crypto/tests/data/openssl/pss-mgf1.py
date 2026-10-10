"""Writes pss-mgf1.txt beside this file: RSASSA-PSS signatures whose MGF1 hash
is not their message hash, which no published vector set this crate carries
has (NIST's SigVerPSS and RSA Laboratories' pss-vect.txt both use one hash for
both).

Run once, on 3 October 2026, with OpenSSL 3.0.13; the output is committed and
nothing re-runs this. OpenSSL makes the key and the signatures; this aid only
prints them as hex -- it adjudicates nothing (ruling 13). A regeneration
produces different bytes, because the key is fresh each time.

    python3 pss-mgf1.py <work-dir>
"""

import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
WORK = sys.argv[1]
os.makedirs(WORK, exist_ok=True)

MESSAGE = b"tinker-pdf: an RSASSA-PSS signature whose mask hash is not its message hash"

# (message hash, MGF1 hash, salt length): each mask hash differs from its
# message hash, in both directions of output length.
CASES = [
    ("sha256", "sha1", 32),
    ("sha1", "sha256", 20),
    ("sha512", "sha256", 64),
    ("sha384", "sha512", 48),
]


def run(*args):
    print("$", " ".join(args))
    return subprocess.run(args, check=True, capture_output=True).stdout


def w(name):
    return os.path.join(WORK, name)


version = " ".join(run("openssl", "version").decode().split()[:5])
run("openssl", "genpkey", "-algorithm", "RSA", "-pkeyopt", "rsa_keygen_bits:2048",
    "-out", w("key.pem"))
run("openssl", "pkey", "-in", w("key.pem"), "-pubout", "-out", w("pub.pem"))
modulus = run("openssl", "rsa", "-in", w("key.pem"), "-noout", "-modulus").decode()
modulus = modulus.strip().split("=", 1)[1].lower()
with open(w("msg.bin"), "wb") as f:
    f.write(MESSAGE)

lines = [
    "# RSASSA-PSS (RFC 8017 8.1) with an MGF1 hash other than the message hash.",
    "# Made once on 2026-10-03 by " + version + ", through pss-mgf1.py beside",
    "# this file; for each case below:",
    "#   openssl dgst -<hash> -sign key.pem -sigopt rsa_padding_mode:pss \\",
    "#     -sigopt rsa_pss_saltlen:<salt> -sigopt rsa_mgf1_md:<mgf1> msg.bin",
    "# The private key was not kept. Msg is the bytes signed, in hex.",
    "",
    "n = " + modulus,
    "e = 010001",
    "Msg = " + MESSAGE.hex(),
]
for hash_name, mask_name, salt in CASES:
    signature_path = w("sig-%s-%s.bin" % (hash_name, mask_name))
    run("openssl", "dgst", "-" + hash_name, "-sign", w("key.pem"),
        "-sigopt", "rsa_padding_mode:pss", "-sigopt", "rsa_pss_saltlen:%d" % salt,
        "-sigopt", "rsa_mgf1_md:" + mask_name, "-out", signature_path, w("msg.bin"))
    with open(signature_path, "rb") as f:
        signature = f.read()
    lines += [
        "",
        "Hash = " + hash_name.upper(),
        "MGF1 = " + mask_name.upper(),
        "SaltLength = %d" % salt,
        "S = " + signature.hex(),
    ]

with open(os.path.join(HERE, "pss-mgf1.txt"), "w") as f:
    f.write("\n".join(lines) + "\n")
