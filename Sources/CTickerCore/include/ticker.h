#ifndef TICKER_H
#define TICKER_H
#include <stddef.h>
#include <stdint.h>

typedef struct Ticker Ticker;
// All calls for a handle must be serialized. Times are Unix seconds, positions stay in AppKit points.
// frontmost is a JSON string: "wow", "ticker" or "other". Bad settings retain the app's defaults policy.
// On failure create returns NULL and sets error to an owned JSON {"error":...} string.
Ticker *ticker_create(const char *settings, const char *frontmost, double now, char **error);
// Each returned string is owned by the caller and must be freed, including error responses.
char *ticker_handle(Ticker *ticker, const char *input);
// max_bars == 0 requests metadata only; utc_offset is seconds east of UTC at chartStart.
char *ticker_view(const Ticker *ticker, size_t max_bars, int32_t utc_offset);
void ticker_string_free(char *value);
void ticker_destroy(Ticker *ticker);
#endif
