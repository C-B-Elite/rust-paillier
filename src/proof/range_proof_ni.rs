use ring::digest::{Context, SHA256};
use std::borrow::Borrow;
use std::fmt;
use std::error::Error;
use proof::range_proof::{ChallengeBits, EncryptedPairs, Proof};
use {BigInt, EncryptionKey, Paillier, RawCiphertext};
use proof::RangeProof;

/// Zero-knowledge range proof that a value x<q/3 lies in interval [0,q].
///
/// The verifier is given only c = ENC(ek,x).
/// The prover has input x, dk, r (randomness used for calculating c)
/// It is assumed that q is known to both.
///
/// References:
/// - Appendix A in [Lindell'17](https://eprint.iacr.org/2017/552)
/// - Section 1.2.2 in [Boudot '00](https://www.iacr.org/archive/eurocrypt2000/1807/18070437-new.pdf)
///
/// This is a non-interactive version of the proof, using Fiat Shamir Transform and assuming Random Oracle Model

// TODO: use error chain
#[derive(Debug)]
pub struct RangeProofError;

impl fmt::Display for RangeProofError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ProofError")
    }
}

impl Error for RangeProofError {
    fn description(&self) -> &str {
        "range proof error"
    }
}


pub struct RangeProofNi<'a>{
    ek: EncryptionKey,
    range: BigInt,
    ciphertext: RawCiphertext<'a>,
    encrypted_pairs: EncryptedPairs,
    proof: Proof,

}

impl <'a> RangeProofNi<'a>{

    pub fn prove(ek: &EncryptionKey, range: &BigInt, ciphertext: &'a RawCiphertext, secret_x: &BigInt, secret_r: &BigInt) -> RangeProofNi<'a>{
        use proof::RangeProof;
        let (encrypted_pairs, data_randomness_pairs) =
            Paillier::generate_encrypted_pairs(ek, range);
        let (c1, c2) = (encrypted_pairs.c1, encrypted_pairs.c2); // TODO[Morten] fix temporary hack

        let mut vec: Vec<BigInt> = Vec::new();
        vec.push(ek.n.clone());
        vec.extend_from_slice(&c1);
        vec.extend_from_slice(&c2);
        let e = ChallengeBits::from(compute_digest(vec.iter()));

        //assuming digest length > STATISTICAL_ERROR_FACTOR

        let proof =
            Paillier::generate_proof(ek, secret_x, secret_r, &e, range, &data_randomness_pairs);

        RangeProofNi{
            ek: ek.clone(),
            range: range.clone(),
            ciphertext: ciphertext.clone(),
            encrypted_pairs: EncryptedPairs { c1, c2},
            proof,
        }
    }

    pub fn verify(&self, ek: &EncryptionKey, ciphertext: &'a RawCiphertext) -> Result<(), RangeProofError>{
        // make sure proof was done with the same public key
        assert_eq!(ek, &self.ek);
        // make sure proof was done with the same ciphertext
        assert_eq!(ciphertext, &self.ciphertext);
        let mut vec: Vec<BigInt> = Vec::new();
        vec.push(ek.n.clone());
        vec.extend_from_slice(&self.encrypted_pairs.c1);
        vec.extend_from_slice(&self.encrypted_pairs.c2);
        let e = ChallengeBits::from(compute_digest(vec.iter()));
        let result = Paillier::verifier_output(ek, &e, &self.encrypted_pairs, &self.proof, &self.range, &self.ciphertext);
        match result.is_ok() {
            true => Ok(()),
            false => Err(RangeProofError),
        }

    }

    pub fn verify_self(&self) -> Result<(), RangeProofError>{
        let mut vec: Vec<BigInt> = Vec::new();
        vec.push(self.ek.n.clone());
        vec.extend_from_slice(&self.encrypted_pairs.c1);
        vec.extend_from_slice(&self.encrypted_pairs.c2);
        let e = ChallengeBits::from(compute_digest(vec.iter()));
        let result = Paillier::verifier_output(&self.ek, &e, &self.encrypted_pairs, &self.proof, &self.range, &self.ciphertext);
        match result.is_ok() {
            true => Ok(()),
            false => Err(RangeProofError),
        }

    }
}


fn compute_digest<IT>(values: IT) -> Vec<u8>
where
    IT: Iterator,
    IT::Item: Borrow<BigInt>,
{
    let mut digest = Context::new(&SHA256);
    for value in values {
        let bytes: Vec<u8> = value.borrow().into();
        digest.update(&bytes);
    }
    digest.finish().as_ref().into()
}

#[cfg(test)]
mod tests {
    const RANGE_BITS: usize = 256; //for elliptic curves with 256bits for example
    use arithimpl::traits::*;
    use core::*;

    use super::*;
    use test::Bencher;
    use traits::*;
    use {Keypair, RawPlaintext};
    use RangeProofNi;

    fn test_keypair() -> Keypair {
        let p = str::parse("148677972634832330983979593310074301486537017973460461278300587514468301043894574906886127642530475786889672304776052879927627556769456140664043088700743909632312483413393134504352834240399191134336344285483935856491230340093391784574980688823380828143810804684752914935441384845195613674104960646037368551517").unwrap();
        let q = str::parse("158741574437007245654463598139927898730476924736461654463975966787719309357536545869203069369466212089132653564188443272208127277664424448947476335413293018778018615899291704693105620242763173357203898195318179150836424196645745308205164116144020613415407736216097185962171301808761138424668335445923774195463").unwrap();
        Keypair { p, q }
    }

    #[test]
    fn test_prover() {
        let (ek, _dk) = test_keypair().keys();
        let range = BigInt::sample(RANGE_BITS);
        let secret_r = BigInt::sample_below(&ek.n);
        let secret_x = BigInt::sample_below(&range);
        let ciphertext = Paillier::encrypt_with_chosen_randomness(&ek, RawPlaintext::from(&secret_x), &Randomness::from(&secret_r));

        RangeProofNi::prove(&ek, &range, &ciphertext,&secret_x, &secret_r);
    }

    #[test]
    fn test_verifier_for_correct_proof() {
        let (ek, _dk) = test_keypair().keys();
        let range = BigInt::sample(RANGE_BITS);
        let secret_r = BigInt::sample_below(&ek.n);
        let secret_x = BigInt::sample_below(&range.div_floor(&BigInt::from(3)));
        let cipher_x = Paillier::encrypt_with_chosen_randomness(
            &ek,
            RawPlaintext::from(&secret_x),
            &Randomness(secret_r.clone()),
        );
        let range_proof =
            RangeProofNi::prove(&ek, &range, &cipher_x, &secret_x, &secret_r);
            range_proof.verify(&ek, &cipher_x).expect("range proof error");

    }

    #[test]
    #[should_panic]
    fn test_verifier_for_incorrect_proof() {
        let (ek, _dk) = test_keypair().keys();
        let range = BigInt::sample(RANGE_BITS);
        let secret_r = BigInt::sample_below(&ek.n);
        let secret_x = BigInt::sample_range(
            &(BigInt::from(100i32) * &range),
            &(BigInt::from(10000i32) * &range),
        );
        let cipher_x = Paillier::encrypt_with_chosen_randomness(
            &ek,
            RawPlaintext::from(&secret_x),
            &Randomness(secret_r.clone()),
        );
        let range_proof =
            RangeProofNi::prove(&ek, &range, &cipher_x, &secret_x, &secret_r);

            range_proof.verify(&ek, &cipher_x).expect("range proof error");
    }

    #[bench]
    fn bench_range_proof(b: &mut Bencher) {
        // TODO: bench range for 256bit range.
        b.iter(|| {
            let (ek, _dk) = test_keypair().keys();
            let range = BigInt::sample(RANGE_BITS);
            let secret_r = BigInt::sample_below(&ek.n);
            let secret_x = BigInt::sample_below(&range.div_floor(&BigInt::from(3)));
            let cipher_x = Paillier::encrypt_with_chosen_randomness(
                &ek,
                RawPlaintext::from(&secret_x),
                &Randomness(secret_r.clone()),
            );
            let range_proof =
                RangeProofNi::prove(&ek, &range, &cipher_x, &secret_x, &secret_r);

                range_proof.verify(&ek, &cipher_x).expect("range proof error");
        });
    }

}
