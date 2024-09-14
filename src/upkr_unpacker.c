#include <stdint.h>

typedef uint8_t u8;
typedef uint16_t u16;
typedef uint32_t u32;

typedef struct Context {
    u8* upkr_data_ptr;
    u32 upkr_state;
    u8 upkr_probs[1 + 255 + 1 + 2*32 + 2*32];
} Context;

_Static_assert(sizeof(Context) == CONTEXT_SIZE, "");
_Static_assert(_Alignof(Context) == 4, "");

int upkr_decode_bit(Context* cx, int context_index) {
    // shift in a full byte until rANS state is >= 4096
    while(cx->upkr_state < 4096) {
        cx->upkr_state = (cx->upkr_state << 8) | *cx->upkr_data_ptr++;
    }
   
    int prob = cx->upkr_probs[context_index];
    int bit = (cx->upkr_state & 255) < prob ? 1 : 0;
    
    // rANS state and context probability update
    // for the later, add 1/16th (rounded) of difference from either 0 or 256
    if(bit) {
        cx->upkr_state = prob * (cx->upkr_state >> 8) + (cx->upkr_state & 255);
        prob += (256 - prob + 8) >> 4;
    } else {
        cx->upkr_state = (256 - prob) * (cx->upkr_state >> 8) + (cx->upkr_state & 255) - prob;
        prob -= (prob + 8) >> 4;
    }
    cx->upkr_probs[context_index] = prob;

    return bit;
}

int upkr_decode_length(Context* cx, int context_index) {
    int length = 0;
    int bit_pos = 0;
    while(upkr_decode_bit(cx, context_index)) {
        length |= upkr_decode_bit(cx, context_index + 1) << bit_pos++;
        context_index += 2;
    }
    return length | (1 << bit_pos);
}

__attribute__((export_name("upkr_unpack")))
void* upkr_unpack(Context* cx, void* destination, void* compressed_data) {
    cx->upkr_data_ptr = (u8*)compressed_data;
    cx->upkr_state = 0;
    // all contexts are initialized to 128 = equal probability of 0 and 1
    for(int i = 0; i < sizeof(cx->upkr_probs); ++i)
        cx->upkr_probs[i] = 128;
    
    u8* write_ptr = (u8*)destination;
    
    int prev_was_match = 0;
    int offset = 0;
    for(;;) {
        // is match
        if(upkr_decode_bit(cx, 0)) {
            // has offset
            if(prev_was_match || upkr_decode_bit(cx, 256)) {
                offset = upkr_decode_length(cx, 257) - 1;
                if(offset == 0) {
                    // a 0 offset signals the end of the compressed data
                    break;
                }
            }
            int length = upkr_decode_length(cx, 257 + 64);
            while(length--) {
                *write_ptr = write_ptr[-offset];
                ++write_ptr;
            }
            prev_was_match = 1;
        } else {
            // byte contains the previously read bits and indicates the number of
            // read bits by the set top bit. Therefore it can be directly used as the
            // context index. The set top bit ends up at bit position 8 and is not stored.
            int byte = 1;
            while(byte < 256) {
                int bit = upkr_decode_bit(cx, byte);
                byte = (byte << 1) + bit;
            }
            *write_ptr++ = byte;
            prev_was_match = 0;
        }
    }
    
    return write_ptr;
}
