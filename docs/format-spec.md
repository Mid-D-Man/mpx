# MPX Format Specification

## File header (32 bytes)

| Offset | Size | Field         | Notes                              |
|--------|------|---------------|------------------------------------|  
| 0      | 4    | magic         | `0x4D 0x50 0x58 0x00`              |
| 4      | 1    | version       | `1`                                |
| 5      | 1    | color_type    | 0=Gray 1=GrayA 2=RGB 3=RGBA        |
| 6      | 1    | bit_depth     | `8` or `16`                        |
| 7      | 1    | filter_type   | 0=None 1=Sub 2=Up 3=Avg 4=Paeth 5=Adaptive |
| 8      | 4    | width         | u32 LE                             |
| 12     | 4    | height        | u32 LE                             |
| 16     | 1    | channel_count | derived, cached                    |
| 17     | 1    | flags         | bit0 = byte_plane_split            |
| 18     | 14   | reserved      | zeros                              |

## Per-channel block

`[compressed_len: u32 LE][MBFA compressed bytes]`

One block per channel in color_type order (R, G, B, A for RGBA).

## Compression pipeline

1. Channel deinterleave
2. Spatial prediction filter (per row)
3. Byte-plane split (16-bit only — hi/lo byte planes concatenated)
4. MBFA compress
