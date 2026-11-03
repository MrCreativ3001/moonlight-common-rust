// Generates the known-good vector in src/stream/proto/video/depayloader_encryption_tests.rs
// using moonlight-common-c's own wire structs and crypto, then checks that it decrypts again.
//
// From the repository root (needs the submodule and OpenSSL headers):
//   C=moonlight-common-sys/moonlight-common-c; cc -I$C/src -I$C/enet/include -DHAS_SOCKLEN_T \
//     tests/vectors/encrypted_video_vector.c $C/src/PlatformCrypto.c -lcrypto -o /tmp/vector && /tmp/vector
#include <assert.h>
#include <stdio.h>
#include <string.h>

#include "Limelight-internal.h"
#include "Video.h"

// This harness only links PlatformCrypto.c, which needs this for logging
CONNECTION_LISTENER_CALLBACKS ListenerCallbacks;

#define PACKET_SIZE 64
#define PLAINTEXT_SIZE (PACKET_SIZE + MAX_RTP_HEADER_SIZE)

static void print_hex(const char* name, const unsigned char* data, int len) {
    printf("%s = ", name);
    for (int i = 0; i < len; i++) printf("%02x", data[i]);
    printf("\n");
}

int main(void) {
    unsigned char key[16];
    for (int i = 0; i < 16; i++) key[i] = (unsigned char)(0xB0 + i);

    // What Sunshine puts on the wire for the only data shard of an IDR frame
    unsigned char plaintext[PLAINTEXT_SIZE] = {0};
    PRTP_PACKET rtp = (PRTP_PACKET)plaintext;
    rtp->header = 0x80 | FLAG_EXTENSION;
    rtp->sequenceNumber = htons(7);
    rtp->timestamp = htonl(90000);
    PNV_VIDEO_PACKET nv = (PNV_VIDEO_PACKET)(plaintext + MAX_RTP_HEADER_SIZE);
    nv->streamPacketIndex = 7u << 8; // the fields are little endian
    nv->frameIndex = 5;
    nv->flags = FLAG_CONTAINS_PIC_DATA | FLAG_SOF | FLAG_EOF;
    nv->multiFecFlags = 0x10;
    nv->fecInfo = (1u << 22) | (0u << 12);

    unsigned char* payload = (unsigned char*)(nv + 1);
    int payload_len = PACKET_SIZE - (int)sizeof(*nv);
    // The 8 byte frame header: type 0x01, IDR, no latency, last payload length
    payload[0] = 0x01;
    payload[3] = 0x02;
    payload[4] = (unsigned char)payload_len;
    for (int i = 8; i < payload_len; i++) payload[i] = (unsigned char)i;

    // The 5th packet of the stream according to Sunshine's iv construction
    ENC_VIDEO_HEADER header = {0};
    header.iv[0] = 4;
    header.iv[11] = 'V';
    header.frameNumber = 5;

    unsigned char ciphertext[PLAINTEXT_SIZE];
    int len = sizeof(ciphertext);
    PPLT_CRYPTO_CONTEXT ctx = PltCreateCryptoContext();
    int ok = PltEncryptMessage(ctx, ALGORITHM_AES_GCM, 0, key, sizeof(key), header.iv, sizeof(header.iv),
                               header.tag, sizeof(header.tag), plaintext, sizeof(plaintext), ciphertext, &len);
    assert(ok && len == PLAINTEXT_SIZE);

    unsigned char datagram[sizeof(header) + PLAINTEXT_SIZE];
    memcpy(datagram, &header, sizeof(header));
    memcpy(datagram + sizeof(header), ciphertext, PLAINTEXT_SIZE);

    // Decrypt it the way VideoStream.c does
    PENC_VIDEO_HEADER enc = (PENC_VIDEO_HEADER)datagram;
    unsigned char decrypted[PLAINTEXT_SIZE];
    int decrypted_len = sizeof(decrypted);
    ok = PltDecryptMessage(ctx, ALGORITHM_AES_GCM, 0, key, sizeof(key), enc->iv, sizeof(enc->iv), enc->tag,
                           sizeof(enc->tag), (unsigned char*)(enc + 1), PLAINTEXT_SIZE, decrypted, &decrypted_len);
    assert(ok && decrypted_len == PLAINTEXT_SIZE && !memcmp(decrypted, plaintext, PLAINTEXT_SIZE));

    // A flipped ciphertext bit must be rejected
    datagram[sizeof(header) + 3] ^= 1;
    decrypted_len = sizeof(decrypted);
    ok = PltDecryptMessage(ctx, ALGORITHM_AES_GCM, 0, key, sizeof(key), enc->iv, sizeof(enc->iv), enc->tag,
                           sizeof(enc->tag), (unsigned char*)(enc + 1), PLAINTEXT_SIZE, decrypted, &decrypted_len);
    assert(!ok);
    datagram[sizeof(header) + 3] ^= 1;

    print_hex("key", key, sizeof(key));
    print_hex("plaintext", plaintext, sizeof(plaintext));
    print_hex("datagram", datagram, sizeof(datagram));
    PltDestroyCryptoContext(ctx);
    return 0;
}
