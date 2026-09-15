package dev.mcpbridge.mobile.crypto

import org.bouncycastle.crypto.params.Ed25519PrivateKeyParameters
import org.bouncycastle.crypto.params.Ed25519PublicKeyParameters
import org.bouncycastle.crypto.signers.Ed25519Signer

internal actual object Ed25519 {
    actual fun fromSeed(seed: ByteArray): Ed25519KeyPair {
        require(seed.size == 32) { "seed must be 32 bytes, got ${seed.size}" }
        val priv = Ed25519PrivateKeyParameters(seed, 0)
        val pub = priv.generatePublicKey()
        return Ed25519KeyPair(
            privateKey = priv.encoded.copyOf(),
            publicKey = pub.encoded.copyOf(),
        )
    }

    actual fun sign(privateKey: ByteArray, message: ByteArray): ByteArray {
        require(privateKey.size == 32) { "privateKey must be 32 bytes, got ${privateKey.size}" }
        val signer = Ed25519Signer()
        signer.init(true, Ed25519PrivateKeyParameters(privateKey, 0))
        signer.update(message, 0, message.size)
        return signer.generateSignature()
    }

    actual fun verify(
        publicKey: ByteArray,
        message: ByteArray,
        signature: ByteArray,
    ): Boolean {
        require(publicKey.size == 32) { "publicKey must be 32 bytes, got ${publicKey.size}" }
        if (signature.size != 64) return false
        val verifier = Ed25519Signer()
        verifier.init(false, Ed25519PublicKeyParameters(publicKey, 0))
        verifier.update(message, 0, message.size)
        return verifier.verifySignature(signature)
    }
}
