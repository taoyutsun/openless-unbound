#pragma once

#include <msctf.h>
#include <windows.h>

inline constexpr CLSID CLSID_OpenLessTextService = {
    0xc5ea393c,
    0x5644,
    0x490a,
    {0xa3, 0xf1, 0x68, 0x28, 0x43, 0x0e, 0x9b, 0xc6},
};

inline constexpr GUID GUID_OpenLessProfile = {
    0xa3a75150,
    0xb165,
    0x4f0a,
    {0xa2, 0xab, 0xb1, 0xe5, 0x9d, 0x76, 0xa5, 0xf8},
};

inline constexpr wchar_t kOpenLessImeName[] = L"OpenLess Unbound Voice Input";
inline constexpr LANGID kOpenLessLangId = 0x0404;
