#ifndef XRACK_NATIVE_BRIDGE_H
#define XRACK_NATIVE_BRIDGE_H
#include <stddef.h>
#ifdef __cplusplus
extern "C" {
#endif
typedef struct XrackBuffer {
  unsigned char *data;
  size_t len;
} XrackBuffer;
// Inputs are borrowed. Results must be released by the matching free.
XrackBuffer xrack_parse(const unsigned char *source, size_t len);
void xrack_parse_free(XrackBuffer buffer);
#ifdef __cplusplus
}
#endif
#endif
