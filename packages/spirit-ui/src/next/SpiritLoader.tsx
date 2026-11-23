import { useEffect, useState } from "react";

// Sprite data below is generated from design/brand/mark/grid.json (rows run-length encoded into
// SVG paths); the pixel art is the owner's exact StackSpirit sprout. Do not edit by hand.
const SPRITE_W = 101;
const SPRITE_H = 124;
const SPRITE_PALETTE: Record<string, string> = {"o": "#0B3720", "d": "#1A5B38", "s": "#2A7C44", "m": "#47A159", "l": "#78C462", "p": "#C4E9A0", "e": "#080C0B", "w": "#F6F2E8"};
const SPRITE_EYES = [{"top": 31, "bottom": 46, "left": 21, "right": 41}, {"top": 31, "bottom": 46, "left": 62, "right": 83}] as const;
const SPRITE_LAYERS: Record<string, Record<string, string>> = {
 "body": {
  "o": "M28 24h7v1h-7zM49 24h1v1h-1zM52 24h1v1h-1zM67 24h7v1h-7zM25 25h3v1h-3zM35 25h3v1h-3zM49 25h1v1h-1zM52 25h1v1h-1zM64 25h3v1h-3zM74 25h3v1h-3zM23 26h2v1h-2zM38 26h2v1h-2zM49 26h1v1h-1zM52 26h1v1h-1zM62 26h2v1h-2zM77 26h3v1h-3zM21 27h2v1h-2zM40 27h1v1h-1zM49 27h1v1h-1zM52 27h1v1h-1zM61 27h2v1h-2zM79 27h2v1h-2zM20 28h2v1h-2zM41 28h1v1h-1zM48 28h2v1h-2zM52 28h2v1h-2zM60 28h2v1h-2zM80 28h2v1h-2zM19 29h2v1h-2zM42 29h1v1h-1zM45 29h4v1h-4zM53 29h4v1h-4zM59 29h2v1h-2zM81 29h2v1h-2zM18 30h2v1h-2zM42 30h3v1h-3zM54 30h6v1h-6zM82 30h1v1h-1zM18 31h1v1h-1zM43 31h1v1h-1zM58 31h1v1h-1zM83 31h1v1h-1zM17 32h1v1h-1zM44 32h1v1h-1zM57 32h1v1h-1zM84 32h1v1h-1zM17 33h1v1h-1zM44 33h1v1h-1zM57 33h1v1h-1zM84 33h1v1h-1zM16 34h1v1h-1zM44 34h2v1h-2zM57 34h1v1h-1zM84 34h2v1h-2zM16 35h1v1h-1zM45 35h1v1h-1zM56 35h1v1h-1zM85 35h1v1h-1zM16 36h1v1h-1zM45 36h1v1h-1zM85 36h1v1h-1zM15 37h2v1h-2zM85 37h2v1h-2zM15 38h2v1h-2zM85 38h2v1h-2zM15 39h2v1h-2zM85 39h2v1h-2zM15 40h2v1h-2zM85 40h2v1h-2zM15 41h2v1h-2zM85 41h2v1h-2zM15 42h2v1h-2zM85 42h2v1h-2zM14 43h2v1h-2zM86 43h2v1h-2zM13 44h2v1h-2zM87 44h2v1h-2zM12 45h2v1h-2zM88 45h2v1h-2zM11 46h2v1h-2zM89 46h2v1h-2zM10 47h2v1h-2zM90 47h2v1h-2zM9 48h2v1h-2zM91 48h1v1h-1zM8 49h2v1h-2zM91 49h2v1h-2zM8 50h2v1h-2zM92 50h1v1h-1zM7 51h2v1h-2zM92 51h2v1h-2zM6 52h2v1h-2zM94 52h1v1h-1zM6 53h1v1h-1zM94 53h1v1h-1zM5 54h2v1h-2zM95 54h1v1h-1zM5 55h1v1h-1zM95 55h1v1h-1zM4 56h1v1h-1zM96 56h1v1h-1zM4 57h1v1h-1zM96 57h1v1h-1zM3 58h2v1h-2zM96 58h2v1h-2zM3 59h1v1h-1zM97 59h1v1h-1zM3 60h1v1h-1zM97 60h1v1h-1zM2 61h2v1h-2zM97 61h2v1h-2zM2 62h1v1h-1zM98 62h1v1h-1zM2 63h1v1h-1zM98 63h1v1h-1zM2 64h1v1h-1zM44 64h14v1h-14zM98 64h1v1h-1zM1 65h2v1h-2zM43 65h2v1h-2zM57 65h2v1h-2zM98 65h2v1h-2zM1 66h1v1h-1zM99 66h1v1h-1zM1 67h1v1h-1zM99 67h1v1h-1zM1 68h1v1h-1zM99 68h1v1h-1zM1 69h1v1h-1zM99 69h2v1h-2zM0 70h2v1h-2zM100 70h1v1h-1zM0 71h2v1h-2zM100 71h1v1h-1zM0 72h2v1h-2zM100 72h1v1h-1zM0 73h2v1h-2zM100 73h1v1h-1zM0 74h2v1h-2zM100 74h1v1h-1zM0 75h2v1h-2zM100 75h1v1h-1zM0 76h2v1h-2zM100 76h1v1h-1zM0 77h2v1h-2zM100 77h1v1h-1zM0 78h2v1h-2zM100 78h1v1h-1zM0 79h2v1h-2zM100 79h1v1h-1zM0 80h2v1h-2zM100 80h1v1h-1zM0 81h2v1h-2zM100 81h1v1h-1zM0 82h2v1h-2zM100 82h1v1h-1zM0 83h2v1h-2zM100 83h1v1h-1zM1 84h1v1h-1zM99 84h1v1h-1zM1 85h2v1h-2zM99 85h1v1h-1zM1 86h2v1h-2zM99 86h1v1h-1zM1 87h2v1h-2zM99 87h1v1h-1zM2 88h2v1h-2zM98 88h1v1h-1zM2 89h2v1h-2zM98 89h1v1h-1zM2 90h2v1h-2zM98 90h1v1h-1zM2 91h3v1h-3zM97 91h2v1h-2zM3 92h2v1h-2zM97 92h1v1h-1zM4 93h1v1h-1zM97 93h1v1h-1zM4 94h2v1h-2zM95 94h2v1h-2zM5 95h1v1h-1zM95 95h2v1h-2zM5 96h2v1h-2zM95 96h2v1h-2zM5 97h2v1h-2zM94 97h2v1h-2zM6 98h2v1h-2zM94 98h1v1h-1zM6 99h3v1h-3zM93 99h2v1h-2zM7 100h2v1h-2zM93 100h1v1h-1zM7 101h3v1h-3zM92 101h1v1h-1zM8 102h2v1h-2zM91 102h2v1h-2zM9 103h2v1h-2zM91 103h1v1h-1zM10 104h2v1h-2zM90 104h1v1h-1zM11 105h1v1h-1zM89 105h1v1h-1zM11 106h3v1h-3zM88 106h2v1h-2zM12 107h2v1h-2zM87 107h2v1h-2zM13 108h2v1h-2zM86 108h2v1h-2zM14 109h2v1h-2zM85 109h2v1h-2zM15 110h2v1h-2zM84 110h2v1h-2zM16 111h2v1h-2zM83 111h2v1h-2zM18 112h2v1h-2zM82 112h2v1h-2zM20 113h1v1h-1zM81 113h2v1h-2zM21 114h2v1h-2zM79 114h2v1h-2zM23 115h1v1h-1zM78 115h1v1h-1zM24 116h3v1h-3zM76 116h2v1h-2zM26 117h2v1h-2zM73 117h3v1h-3zM28 118h2v1h-2zM71 118h3v1h-3zM30 119h3v1h-3zM69 119h3v1h-3zM33 120h3v1h-3zM66 120h3v1h-3zM36 121h4v1h-4zM62 121h4v1h-4zM40 122h5v1h-5zM57 122h6v1h-6zM44 123h14v1h-14z",
  "l": "M50 24h2v1h-2zM33 25h2v1h-2zM50 25h2v1h-2zM72 25h2v1h-2zM33 26h5v1h-5zM50 26h2v1h-2zM72 26h5v1h-5zM29 27h11v1h-11zM50 27h2v1h-2zM68 27h11v1h-11zM25 28h16v1h-16zM50 28h2v1h-2zM65 28h13v1h-13zM79 28h1v1h-1zM24 29h18v1h-18zM49 29h4v1h-4zM64 29h15v1h-15zM80 29h1v1h-1zM22 30h20v1h-20zM45 30h9v1h-9zM60 30h1v1h-1zM62 30h16v1h-16zM79 30h1v1h-1zM81 30h1v1h-1zM21 31h22v1h-22zM45 31h12v1h-12zM59 31h22v1h-22zM82 31h1v1h-1zM20 32h24v1h-24zM58 32h24v1h-24zM18 33h26v1h-26zM58 33h26v1h-26zM17 34h27v1h-27zM58 34h26v1h-26zM17 35h26v1h-26zM57 35h28v1h-28zM17 36h26v1h-26zM56 36h1v1h-1zM58 36h27v1h-27zM17 37h25v1h-25zM57 37h1v1h-1zM59 37h25v1h-25zM17 38h1v1h-1zM19 38h23v1h-23zM58 38h1v1h-1zM60 38h24v1h-24zM18 39h1v1h-1zM20 39h22v1h-22zM57 39h1v1h-1zM59 39h1v1h-1zM61 39h23v1h-23zM17 40h1v1h-1zM19 40h23v1h-23zM58 40h1v1h-1zM60 40h24v1h-24zM18 41h1v1h-1zM20 41h22v1h-22zM59 41h1v1h-1zM61 41h22v1h-22zM19 42h1v1h-1zM21 42h20v1h-20zM62 42h21v1h-21zM20 43h1v1h-1zM22 43h18v1h-18zM61 43h21v1h-21zM15 44h1v1h-1zM22 44h17v1h-17zM63 44h18v1h-18zM14 45h2v1h-2zM23 45h15v1h-15zM63 45h17v1h-17zM13 46h4v1h-4zM25 46h12v1h-12zM66 46h13v1h-13zM12 47h6v1h-6zM33 47h2v1h-2zM68 47h9v1h-9zM11 48h7v1h-7zM10 49h4v1h-4zM16 49h3v1h-3zM10 50h3v1h-3zM17 50h4v1h-4zM9 51h3v1h-3zM19 51h3v1h-3zM35 51h1v1h-1zM8 52h3v1h-3zM18 52h6v1h-6zM32 52h2v1h-2zM7 53h4v1h-4zM17 53h16v1h-16zM7 54h3v1h-3zM15 54h16v1h-16zM6 55h3v1h-3zM13 55h17v1h-17zM5 56h4v1h-4zM12 56h10v1h-10zM5 57h3v1h-3zM11 57h10v1h-10zM5 58h3v1h-3zM10 58h10v1h-10zM21 58h1v1h-1zM4 59h4v1h-4zM9 59h9v1h-9zM20 59h1v1h-1zM4 60h3v1h-3zM9 60h7v1h-7zM4 61h3v1h-3zM8 61h5v1h-5zM14 61h1v1h-1zM3 62h8v1h-8zM12 62h1v1h-1zM3 63h8v1h-8zM3 64h7v1h-7zM11 64h1v1h-1zM3 65h6v1h-6zM10 65h1v1h-1zM2 66h4v1h-4zM7 66h1v1h-1zM9 66h1v1h-1zM2 67h3v1h-3zM6 67h1v1h-1zM8 67h1v1h-1zM2 68h2v1h-2zM5 68h1v1h-1zM7 68h1v1h-1zM2 69h3v1h-3zM6 69h1v1h-1zM2 70h2v1h-2zM5 70h1v1h-1zM7 70h1v1h-1zM2 71h3v1h-3zM6 71h1v1h-1zM2 72h2v1h-2zM5 72h1v1h-1zM2 73h1v1h-1zM4 73h1v1h-1zM2 74h2v1h-2zM2 75h1v1h-1zM2 76h2v1h-2zM2 77h1v1h-1z",
  "p": "M28 25h5v1h-5zM67 25h5v1h-5zM25 26h8v1h-8zM64 26h8v1h-8zM23 27h6v1h-6zM63 27h5v1h-5zM22 28h3v1h-3zM62 28h3v1h-3zM21 29h3v1h-3zM61 29h3v1h-3zM20 30h2v1h-2zM61 30h1v1h-1zM19 31h2v1h-2zM18 32h2v1h-2zM14 49h2v1h-2zM13 50h4v1h-4zM12 51h7v1h-7zM11 52h7v1h-7zM11 53h6v1h-6zM10 54h5v1h-5zM9 55h4v1h-4zM9 56h3v1h-3zM8 57h3v1h-3zM8 58h2v1h-2zM8 59h1v1h-1zM7 60h2v1h-2zM7 61h1v1h-1z",
  "m": "M78 28h1v1h-1zM79 29h1v1h-1zM78 30h1v1h-1zM80 30h1v1h-1zM44 31h1v1h-1zM57 31h1v1h-1zM81 31h1v1h-1zM45 32h12v1h-12zM82 32h2v1h-2zM45 33h12v1h-12zM46 34h11v1h-11zM43 35h2v1h-2zM46 35h10v1h-10zM43 36h2v1h-2zM46 36h10v1h-10zM57 36h1v1h-1zM42 37h2v1h-2zM46 37h11v1h-11zM58 37h1v1h-1zM84 37h1v1h-1zM18 38h1v1h-1zM42 38h3v1h-3zM46 38h12v1h-12zM59 38h1v1h-1zM84 38h1v1h-1zM17 39h1v1h-1zM19 39h1v1h-1zM43 39h1v1h-1zM45 39h12v1h-12zM58 39h1v1h-1zM60 39h1v1h-1zM84 39h1v1h-1zM18 40h1v1h-1zM42 40h1v1h-1zM45 40h13v1h-13zM59 40h1v1h-1zM84 40h1v1h-1zM17 41h1v1h-1zM19 41h1v1h-1zM45 41h12v1h-12zM60 41h1v1h-1zM83 41h2v1h-2zM17 42h2v1h-2zM20 42h1v1h-1zM45 42h12v1h-12zM60 42h2v1h-2zM83 42h2v1h-2zM17 43h3v1h-3zM21 43h1v1h-1zM44 43h13v1h-13zM82 43h3v1h-3zM17 44h5v1h-5zM43 44h15v1h-15zM62 44h1v1h-1zM81 44h3v1h-3zM18 45h5v1h-5zM42 45h16v1h-16zM80 45h3v1h-3zM19 46h6v1h-6zM41 46h18v1h-18zM79 46h3v1h-3zM20 47h13v1h-13zM40 47h21v1h-21zM67 47h1v1h-1zM77 47h3v1h-3zM21 48h1v1h-1zM23 48h10v1h-10zM39 48h23v1h-23zM69 48h8v1h-8zM80 48h1v1h-1zM23 49h1v1h-1zM25 49h1v1h-1zM27 49h1v1h-1zM29 49h1v1h-1zM31 49h1v1h-1zM38 49h26v1h-26zM71 49h5v1h-5zM77 49h1v1h-1zM79 49h1v1h-1zM26 50h1v1h-1zM28 50h1v1h-1zM36 50h30v1h-30zM72 50h1v1h-1zM74 50h1v1h-1zM76 50h1v1h-1zM36 51h32v1h-32zM34 52h37v1h-37zM82 52h2v1h-2zM85 52h1v1h-1zM33 53h40v1h-40zM79 53h6v1h-6zM31 54h56v1h-56zM30 55h56v1h-56zM87 55h1v1h-1zM22 56h65v1h-65zM21 57h67v1h-67zM20 58h1v1h-1zM22 58h67v1h-67zM18 59h2v1h-2zM21 59h67v1h-67zM89 59h1v1h-1zM16 60h73v1h-73zM91 60h1v1h-1zM13 61h1v1h-1zM15 61h74v1h-74zM11 62h1v1h-1zM13 62h76v1h-76zM11 63h79v1h-79zM10 64h1v1h-1zM12 64h32v1h-32zM58 64h33v1h-33zM9 65h1v1h-1zM11 65h32v1h-32zM45 65h12v1h-12zM59 65h31v1h-31zM6 66h1v1h-1zM8 66h1v1h-1zM10 66h80v1h-80zM5 67h1v1h-1zM7 67h1v1h-1zM9 67h80v1h-80zM4 68h1v1h-1zM6 68h1v1h-1zM8 68h80v1h-80zM89 68h1v1h-1zM5 69h1v1h-1zM7 69h81v1h-81zM91 69h1v1h-1zM4 70h1v1h-1zM6 70h1v1h-1zM8 70h81v1h-81zM5 71h1v1h-1zM7 71h81v1h-81zM89 71h1v1h-1zM4 72h1v1h-1zM6 72h82v1h-82zM3 73h1v1h-1zM5 73h83v1h-83zM89 73h1v1h-1zM4 74h84v1h-84zM90 74h1v1h-1zM3 75h84v1h-84zM88 75h1v1h-1zM4 76h82v1h-82zM87 76h1v1h-1zM3 77h84v1h-84zM2 78h84v1h-84zM88 78h1v1h-1zM2 79h84v1h-84zM2 80h82v1h-82zM86 80h1v1h-1zM2 81h81v1h-81zM84 81h1v1h-1zM2 82h80v1h-80zM83 82h1v1h-1zM85 82h1v1h-1zM2 83h79v1h-79zM82 83h1v1h-1zM84 83h1v1h-1zM2 84h80v1h-80zM83 84h1v1h-1zM3 85h79v1h-79zM3 86h76v1h-76zM80 86h1v1h-1zM3 87h75v1h-75zM79 87h1v1h-1zM81 87h1v1h-1zM4 88h73v1h-73zM78 88h1v1h-1zM80 88h1v1h-1zM82 88h1v1h-1zM4 89h74v1h-74zM79 89h1v1h-1zM81 89h1v1h-1zM4 90h73v1h-73zM78 90h1v1h-1zM80 90h1v1h-1zM5 91h69v1h-69zM75 91h1v1h-1zM77 91h1v1h-1zM5 92h68v1h-68zM74 92h1v1h-1zM76 92h1v1h-1zM78 92h1v1h-1zM5 93h67v1h-67zM73 93h1v1h-1zM75 93h1v1h-1zM77 93h1v1h-1zM6 94h67v1h-67zM74 94h1v1h-1zM6 95h63v1h-63zM70 95h1v1h-1zM7 96h63v1h-63zM71 96h1v1h-1zM7 97h62v1h-62zM70 97h1v1h-1zM8 98h58v1h-58zM67 98h1v1h-1zM69 98h1v1h-1zM10 99h52v1h-52zM63 99h1v1h-1zM70 99h1v1h-1zM9 100h1v1h-1zM11 100h50v1h-50zM62 100h1v1h-1zM64 100h1v1h-1zM12 101h48v1h-48zM66 101h1v1h-1zM15 102h42v1h-42zM58 102h1v1h-1zM60 102h1v1h-1zM65 102h1v1h-1zM67 102h1v1h-1zM16 103h40v1h-40zM57 103h1v1h-1zM59 103h1v1h-1zM64 103h1v1h-1zM15 104h2v1h-2zM19 104h33v1h-33zM53 104h1v1h-1zM56 104h1v1h-1zM58 104h1v1h-1zM61 104h1v1h-1zM63 104h1v1h-1zM16 105h1v1h-1zM19 105h1v1h-1zM22 105h29v1h-29zM55 105h1v1h-1zM57 105h1v1h-1zM60 105h1v1h-1zM19 106h1v1h-1zM25 106h23v1h-23zM49 106h1v1h-1zM56 106h1v1h-1zM21 107h1v1h-1zM26 107h1v1h-1zM28 107h15v1h-15zM44 107h1v1h-1zM46 107h1v1h-1zM48 107h1v1h-1zM50 107h1v1h-1zM53 107h1v1h-1zM55 107h1v1h-1zM33 108h1v1h-1zM35 108h3v1h-3zM39 108h1v1h-1zM41 108h1v1h-1zM43 108h1v1h-1zM45 108h1v1h-1zM47 108h1v1h-1zM49 108h1v1h-1zM36 109h1v1h-1zM38 109h1v1h-1zM40 109h1v1h-1zM42 109h1v1h-1zM44 109h1v1h-1zM46 109h1v1h-1zM35 110h1v1h-1zM37 110h1v1h-1zM39 110h1v1h-1zM41 110h1v1h-1zM34 111h1v1h-1zM36 111h1v1h-1zM38 111h1v1h-1zM40 111h1v1h-1z",
  "e": "M83 30h1v1h-1zM17 31h1v1h-1zM84 31h1v1h-1zM15 36h1v1h-1zM86 36h1v1h-1zM3 93h1v1h-1zM4 95h1v1h-1zM9 104h1v1h-1zM10 105h1v1h-1zM90 105h1v1h-1z",
  "s": "M44 37h2v1h-2zM45 38h1v1h-1zM42 39h1v1h-1zM44 39h1v1h-1zM43 40h2v1h-2zM42 41h3v1h-3zM57 41h2v1h-2zM41 42h4v1h-4zM57 42h3v1h-3zM16 43h1v1h-1zM40 43h4v1h-4zM57 43h4v1h-4zM39 44h4v1h-4zM58 44h4v1h-4zM86 44h1v1h-1zM17 45h1v1h-1zM38 45h4v1h-4zM58 45h5v1h-5zM86 45h2v1h-2zM17 46h2v1h-2zM37 46h4v1h-4zM59 46h7v1h-7zM82 46h1v1h-1zM85 46h4v1h-4zM18 47h2v1h-2zM35 47h5v1h-5zM61 47h6v1h-6zM80 47h2v1h-2zM84 47h6v1h-6zM18 48h3v1h-3zM22 48h1v1h-1zM33 48h6v1h-6zM62 48h7v1h-7zM77 48h3v1h-3zM81 48h1v1h-1zM83 48h8v1h-8zM19 49h4v1h-4zM24 49h1v1h-1zM26 49h1v1h-1zM28 49h1v1h-1zM30 49h1v1h-1zM32 49h6v1h-6zM64 49h7v1h-7zM76 49h1v1h-1zM78 49h1v1h-1zM80 49h11v1h-11zM21 50h5v1h-5zM27 50h1v1h-1zM29 50h7v1h-7zM66 50h6v1h-6zM73 50h1v1h-1zM75 50h1v1h-1zM77 50h15v1h-15zM22 51h13v1h-13zM68 51h24v1h-24zM24 52h8v1h-8zM71 52h11v1h-11zM84 52h1v1h-1zM86 52h8v1h-8zM73 53h6v1h-6zM85 53h9v1h-9zM87 54h8v1h-8zM86 55h1v1h-1zM88 55h7v1h-7zM87 56h9v1h-9zM88 57h8v1h-8zM89 58h7v1h-7zM88 59h1v1h-1zM90 59h7v1h-7zM89 60h2v1h-2zM92 60h5v1h-5zM89 61h8v1h-8zM89 62h9v1h-9zM90 63h8v1h-8zM91 64h7v1h-7zM90 65h8v1h-8zM90 66h9v1h-9zM89 67h10v1h-10zM88 68h1v1h-1zM90 68h9v1h-9zM88 69h3v1h-3zM92 69h7v1h-7zM89 70h11v1h-11zM88 71h1v1h-1zM90 71h10v1h-10zM88 72h12v1h-12zM88 73h1v1h-1zM90 73h10v1h-10zM88 74h2v1h-2zM91 74h9v1h-9zM87 75h1v1h-1zM89 75h9v1h-9zM86 76h1v1h-1zM88 76h10v1h-10zM87 77h11v1h-11zM86 78h2v1h-2zM89 78h8v1h-8zM86 79h10v1h-10zM97 79h1v1h-1zM84 80h2v1h-2zM87 80h8v1h-8zM96 80h1v1h-1zM83 81h1v1h-1zM85 81h11v1h-11zM82 82h1v1h-1zM84 82h1v1h-1zM86 82h9v1h-9zM96 82h1v1h-1zM81 83h1v1h-1zM83 83h1v1h-1zM85 83h9v1h-9zM95 83h1v1h-1zM82 84h1v1h-1zM84 84h11v1h-11zM82 85h12v1h-12zM95 85h1v1h-1zM79 86h1v1h-1zM81 86h12v1h-12zM94 86h1v1h-1zM78 87h1v1h-1zM80 87h1v1h-1zM82 87h10v1h-10zM93 87h1v1h-1zM77 88h1v1h-1zM79 88h1v1h-1zM81 88h1v1h-1zM83 88h10v1h-10zM94 88h1v1h-1zM78 89h1v1h-1zM80 89h1v1h-1zM82 89h10v1h-10zM93 89h1v1h-1zM77 90h1v1h-1zM79 90h1v1h-1zM81 90h10v1h-10zM92 90h1v1h-1zM74 91h1v1h-1zM76 91h1v1h-1zM78 91h12v1h-12zM91 91h1v1h-1zM73 92h1v1h-1zM75 92h1v1h-1zM77 92h1v1h-1zM79 92h10v1h-10zM90 92h1v1h-1zM72 93h1v1h-1zM74 93h1v1h-1zM76 93h1v1h-1zM78 93h10v1h-10zM73 94h1v1h-1zM75 94h13v1h-13zM69 95h1v1h-1zM71 95h15v1h-15zM88 95h1v1h-1zM70 96h1v1h-1zM72 96h13v1h-13zM69 97h1v1h-1zM71 97h14v1h-14zM86 97h1v1h-1zM66 98h1v1h-1zM68 98h1v1h-1zM70 98h15v1h-15zM86 98h1v1h-1zM9 99h1v1h-1zM62 99h1v1h-1zM64 99h6v1h-6zM71 99h10v1h-10zM82 99h3v1h-3zM10 100h1v1h-1zM61 100h1v1h-1zM63 100h1v1h-1zM65 100h15v1h-15zM81 100h1v1h-1zM83 100h1v1h-1zM10 101h2v1h-2zM60 101h6v1h-6zM67 101h14v1h-14zM82 101h1v1h-1zM10 102h5v1h-5zM57 102h1v1h-1zM59 102h1v1h-1zM61 102h4v1h-4zM66 102h1v1h-1zM68 102h12v1h-12zM81 102h1v1h-1zM11 103h5v1h-5zM56 103h1v1h-1zM58 103h1v1h-1zM60 103h4v1h-4zM65 103h12v1h-12zM78 103h1v1h-1zM80 103h1v1h-1zM12 104h3v1h-3zM17 104h2v1h-2zM52 104h1v1h-1zM54 104h2v1h-2zM57 104h1v1h-1zM59 104h2v1h-2zM62 104h1v1h-1zM64 104h14v1h-14zM79 104h1v1h-1zM12 105h4v1h-4zM17 105h2v1h-2zM20 105h2v1h-2zM51 105h4v1h-4zM56 105h1v1h-1zM58 105h2v1h-2zM61 105h12v1h-12zM74 105h3v1h-3zM78 105h1v1h-1zM14 106h5v1h-5zM20 106h5v1h-5zM48 106h1v1h-1zM50 106h6v1h-6zM57 106h15v1h-15zM73 106h1v1h-1zM75 106h1v1h-1zM14 107h7v1h-7zM22 107h4v1h-4zM27 107h1v1h-1zM43 107h1v1h-1zM45 107h1v1h-1zM47 107h1v1h-1zM49 107h1v1h-1zM51 107h2v1h-2zM54 107h1v1h-1zM56 107h17v1h-17zM74 107h1v1h-1zM76 107h1v1h-1zM15 108h18v1h-18zM34 108h1v1h-1zM38 108h1v1h-1zM40 108h1v1h-1zM42 108h1v1h-1zM44 108h1v1h-1zM46 108h1v1h-1zM48 108h1v1h-1zM50 108h19v1h-19zM70 108h2v1h-2zM73 108h1v1h-1zM16 109h20v1h-20zM37 109h1v1h-1zM39 109h1v1h-1zM41 109h1v1h-1zM43 109h1v1h-1zM45 109h1v1h-1zM47 109h21v1h-21zM69 109h1v1h-1zM72 109h1v1h-1zM17 110h18v1h-18zM36 110h1v1h-1zM38 110h1v1h-1zM40 110h1v1h-1zM42 110h25v1h-25zM68 110h1v1h-1zM70 110h2v1h-2zM18 111h16v1h-16zM35 111h1v1h-1zM37 111h1v1h-1zM39 111h1v1h-1zM41 111h23v1h-23zM65 111h1v1h-1zM67 111h1v1h-1zM69 111h1v1h-1zM71 111h1v1h-1zM20 112h43v1h-43zM64 112h1v1h-1zM66 112h1v1h-1zM68 112h1v1h-1zM21 113h39v1h-39zM61 113h1v1h-1zM63 113h1v1h-1zM65 113h1v1h-1zM23 114h34v1h-34zM58 114h1v1h-1zM60 114h1v1h-1zM24 115h3v1h-3zM28 115h28v1h-28zM57 115h1v1h-1zM59 115h1v1h-1zM61 115h1v1h-1zM27 116h1v1h-1zM29 116h1v1h-1zM31 116h22v1h-22zM54 116h1v1h-1zM58 116h1v1h-1zM30 117h1v1h-1zM32 117h1v1h-1zM34 117h13v1h-13zM48 117h1v1h-1zM50 117h1v1h-1zM52 117h1v1h-1zM31 118h1v1h-1zM33 118h1v1h-1zM35 118h1v1h-1zM37 118h1v1h-1zM39 118h1v1h-1zM41 118h1v1h-1zM43 118h1v1h-1zM45 118h1v1h-1zM47 118h1v1h-1zM49 118h1v1h-1zM38 119h1v1h-1zM40 119h1v1h-1zM42 119h1v1h-1zM44 119h1v1h-1z",
  "d": "M85 43h1v1h-1zM16 44h1v1h-1zM84 44h2v1h-2zM16 45h1v1h-1zM83 45h3v1h-3zM83 46h2v1h-2zM82 47h2v1h-2zM82 48h1v1h-1zM98 75h2v1h-2zM98 76h2v1h-2zM98 77h2v1h-2zM97 78h3v1h-3zM96 79h1v1h-1zM98 79h2v1h-2zM95 80h1v1h-1zM97 80h3v1h-3zM96 81h4v1h-4zM95 82h1v1h-1zM97 82h3v1h-3zM94 83h1v1h-1zM96 83h4v1h-4zM95 84h4v1h-4zM94 85h1v1h-1zM96 85h3v1h-3zM93 86h1v1h-1zM95 86h4v1h-4zM92 87h1v1h-1zM94 87h5v1h-5zM93 88h1v1h-1zM95 88h3v1h-3zM92 89h1v1h-1zM94 89h4v1h-4zM91 90h1v1h-1zM93 90h5v1h-5zM90 91h1v1h-1zM92 91h5v1h-5zM89 92h1v1h-1zM91 92h6v1h-6zM88 93h9v1h-9zM88 94h7v1h-7zM86 95h2v1h-2zM89 95h6v1h-6zM85 96h10v1h-10zM85 97h1v1h-1zM87 97h7v1h-7zM85 98h1v1h-1zM87 98h7v1h-7zM81 99h1v1h-1zM85 99h8v1h-8zM80 100h1v1h-1zM82 100h1v1h-1zM84 100h9v1h-9zM81 101h1v1h-1zM83 101h9v1h-9zM80 102h1v1h-1zM82 102h9v1h-9zM77 103h1v1h-1zM79 103h1v1h-1zM81 103h10v1h-10zM78 104h1v1h-1zM80 104h10v1h-10zM73 105h1v1h-1zM77 105h1v1h-1zM79 105h10v1h-10zM72 106h1v1h-1zM74 106h1v1h-1zM76 106h12v1h-12zM73 107h1v1h-1zM75 107h1v1h-1zM77 107h10v1h-10zM69 108h1v1h-1zM72 108h1v1h-1zM74 108h12v1h-12zM68 109h1v1h-1zM70 109h2v1h-2zM73 109h12v1h-12zM67 110h1v1h-1zM69 110h1v1h-1zM72 110h12v1h-12zM64 111h1v1h-1zM66 111h1v1h-1zM68 111h1v1h-1zM70 111h1v1h-1zM72 111h11v1h-11zM63 112h1v1h-1zM65 112h1v1h-1zM67 112h1v1h-1zM69 112h13v1h-13zM60 113h1v1h-1zM62 113h1v1h-1zM64 113h1v1h-1zM66 113h15v1h-15zM57 114h1v1h-1zM59 114h1v1h-1zM61 114h18v1h-18zM27 115h1v1h-1zM56 115h1v1h-1zM58 115h1v1h-1zM60 115h1v1h-1zM62 115h16v1h-16zM28 116h1v1h-1zM30 116h1v1h-1zM53 116h1v1h-1zM55 116h3v1h-3zM59 116h17v1h-17zM28 117h2v1h-2zM31 117h1v1h-1zM33 117h1v1h-1zM47 117h1v1h-1zM49 117h1v1h-1zM51 117h1v1h-1zM53 117h20v1h-20zM30 118h1v1h-1zM32 118h1v1h-1zM34 118h1v1h-1zM36 118h1v1h-1zM38 118h1v1h-1zM40 118h1v1h-1zM42 118h1v1h-1zM44 118h1v1h-1zM46 118h1v1h-1zM48 118h1v1h-1zM50 118h21v1h-21zM33 119h5v1h-5zM39 119h1v1h-1zM41 119h1v1h-1zM43 119h1v1h-1zM45 119h24v1h-24zM36 120h30v1h-30zM40 121h22v1h-22zM45 122h12v1h-12z"
 },
 "stem": {
  "o": "M48 18h1v1h-1zM53 18h2v1h-2zM48 19h2v1h-2zM52 19h2v1h-2zM49 20h1v1h-1zM52 20h1v1h-1zM49 21h1v1h-1zM52 21h1v1h-1zM49 22h1v1h-1zM52 22h1v1h-1zM49 23h1v1h-1zM52 23h1v1h-1z",
  "l": "M49 18h4v1h-4zM50 19h2v1h-2zM50 20h2v1h-2zM50 21h2v1h-2zM50 22h2v1h-2zM50 23h2v1h-2z"
 },
 "leafLeft": {
  "o": "M32 0h8v1h-8zM30 1h2v1h-2zM40 1h4v1h-4zM28 2h2v1h-2zM44 2h2v1h-2zM28 3h1v1h-1zM46 3h1v1h-1zM28 4h1v1h-1zM47 4h1v1h-1zM28 5h1v1h-1zM48 5h1v1h-1zM28 6h2v1h-2zM48 6h2v1h-2zM29 7h1v1h-1zM49 7h1v1h-1zM29 8h2v1h-2zM49 8h1v1h-1zM30 9h1v1h-1zM30 10h2v1h-2zM31 11h1v1h-1zM32 12h1v1h-1zM33 13h1v1h-1zM34 14h2v1h-2zM35 15h3v1h-3zM38 16h3v1h-3zM40 17h8v1h-8z",
  "p": "M32 1h8v1h-8zM30 2h14v1h-14zM31 3h5v1h-5zM41 3h5v1h-5zM44 4h3v1h-3zM47 5h1v1h-1z",
  "l": "M29 3h2v1h-2zM36 3h5v1h-5zM29 4h15v1h-15zM29 5h18v1h-18zM30 6h18v1h-18zM30 7h19v1h-19zM31 8h1v1h-1zM33 8h16v1h-16zM32 9h1v1h-1zM34 9h16v1h-16zM33 10h1v1h-1zM35 10h1v1h-1zM37 10h1v1h-1zM39 10h11v1h-11zM34 11h1v1h-1zM40 11h1v1h-1zM42 11h1v1h-1zM44 11h6v1h-6zM35 12h1v1h-1zM37 12h1v1h-1zM39 12h1v1h-1zM41 12h1v1h-1zM43 12h1v1h-1zM45 12h5v1h-5zM38 13h1v1h-1zM40 13h1v1h-1zM42 13h1v1h-1zM44 13h1v1h-1zM47 13h3v1h-3zM39 14h1v1h-1zM41 14h1v1h-1zM43 14h1v1h-1zM45 14h1v1h-1zM47 14h3v1h-3zM46 15h1v1h-1zM48 15h2v1h-2zM48 16h2v1h-2zM48 17h2v1h-2z",
  "m": "M32 8h1v1h-1zM31 9h1v1h-1zM33 9h1v1h-1zM32 10h1v1h-1zM34 10h1v1h-1zM36 10h1v1h-1zM38 10h1v1h-1zM32 11h2v1h-2zM35 11h5v1h-5zM41 11h1v1h-1zM43 11h1v1h-1zM33 12h2v1h-2zM36 12h1v1h-1zM38 12h1v1h-1zM40 12h1v1h-1zM42 12h1v1h-1zM44 12h1v1h-1zM34 13h4v1h-4zM39 13h1v1h-1zM41 13h1v1h-1zM43 13h1v1h-1zM45 13h2v1h-2zM36 14h3v1h-3zM40 14h1v1h-1zM42 14h1v1h-1zM44 14h1v1h-1zM46 14h1v1h-1zM38 15h8v1h-8zM47 15h1v1h-1zM41 16h7v1h-7z"
 },
 "leafRight": {
  "o": "M63 1h8v1h-8zM59 2h4v1h-4zM71 2h3v1h-3zM57 3h2v1h-2zM73 3h1v1h-1zM55 4h2v1h-2zM73 4h1v1h-1zM54 5h1v1h-1zM73 5h1v1h-1zM53 6h1v1h-1zM72 6h2v1h-2zM53 7h1v1h-1zM72 7h1v1h-1zM50 8h1v1h-1zM52 8h1v1h-1zM71 8h2v1h-2zM50 9h1v1h-1zM52 9h1v1h-1zM71 9h1v1h-1zM52 10h1v1h-1zM71 10h1v1h-1zM51 11h1v1h-1zM70 11h1v1h-1zM50 12h2v1h-2zM69 12h1v1h-1zM50 13h2v1h-2zM68 13h1v1h-1zM66 14h3v1h-3zM65 15h2v1h-2zM61 16h4v1h-4zM54 17h7v1h-7z",
  "p": "M63 2h8v1h-8zM59 3h5v1h-5zM57 4h2v1h-2zM55 5h4v1h-4zM54 6h2v1h-2zM54 7h1v1h-1zM53 8h1v1h-1z",
  "l": "M64 3h9v1h-9zM59 4h14v1h-14zM59 5h14v1h-14zM56 6h16v1h-16zM55 7h17v1h-17zM54 8h17v1h-17zM53 9h16v1h-16zM53 10h13v1h-13zM67 10h1v1h-1zM52 11h11v1h-11zM64 11h1v1h-1zM66 11h1v1h-1zM68 11h1v1h-1zM52 12h10v1h-10zM63 12h1v1h-1zM65 12h1v1h-1zM67 12h1v1h-1zM52 13h5v1h-5zM58 13h1v1h-1zM60 13h1v1h-1zM62 13h1v1h-1zM64 13h1v1h-1zM66 13h1v1h-1zM50 14h6v1h-6zM57 14h1v1h-1zM59 14h1v1h-1zM61 14h1v1h-1zM63 14h1v1h-1zM65 14h1v1h-1zM50 15h5v1h-5zM56 15h1v1h-1zM58 15h1v1h-1zM60 15h1v1h-1zM50 16h4v1h-4zM50 17h4v1h-4z",
  "m": "M69 9h2v1h-2zM66 10h1v1h-1zM68 10h3v1h-3zM63 11h1v1h-1zM65 11h1v1h-1zM67 11h1v1h-1zM69 11h1v1h-1zM62 12h1v1h-1zM64 12h1v1h-1zM66 12h1v1h-1zM68 12h1v1h-1zM57 13h1v1h-1zM59 13h1v1h-1zM61 13h1v1h-1zM63 13h1v1h-1zM65 13h1v1h-1zM67 13h1v1h-1zM56 14h1v1h-1zM58 14h1v1h-1zM60 14h1v1h-1zM62 14h1v1h-1zM64 14h1v1h-1zM55 15h1v1h-1zM57 15h1v1h-1zM59 15h1v1h-1zM61 15h4v1h-4zM54 16h7v1h-7z",
  "e": "M50 10h1v1h-1zM50 11h1v1h-1z"
 },
 "eye0": {
  "e": "M27 31h10v1h-10zM25 32h13v1h-13zM24 33h10v1h-10zM37 33h2v1h-2zM23 34h11v1h-11zM38 34h2v1h-2zM22 35h13v1h-13zM38 35h3v1h-3zM21 36h20v1h-20zM21 37h20v1h-20zM21 38h20v1h-20zM21 39h20v1h-20zM21 40h20v1h-20zM22 41h18v1h-18zM22 42h17v1h-17zM23 43h15v1h-15zM24 44h12v1h-12zM28 45h6v1h-6z",
  "w": "M34 33h3v1h-3zM34 34h4v1h-4zM35 35h3v1h-3z"
 },
 "eye1": {
  "e": "M68 31h10v1h-10zM67 32h12v1h-12zM65 33h11v1h-11zM79 33h1v1h-1zM64 34h11v1h-11zM80 34h1v1h-1zM63 35h14v1h-14zM80 35h2v1h-2zM63 36h19v1h-19zM62 37h21v1h-21zM62 38h21v1h-21zM62 39h21v1h-21zM62 40h20v1h-20zM62 41h20v1h-20zM63 42h18v1h-18zM64 43h16v1h-16zM65 44h14v1h-14zM66 45h12v1h-12z",
  "w": "M76 33h3v1h-3zM75 34h5v1h-5zM77 35h3v1h-3z"
 }
};


function Layer({ name }: { name: string }) {
  const layer = SPRITE_LAYERS[name] ?? {};
  return (
    <>
      {Object.entries(layer).map(([key, d]) => (
        <path key={key} d={d} fill={SPRITE_PALETTE[key]} />
      ))}
    </>
  );
}

/**
 * The StackSpirit loading indicator: the exact pixel-art sprout (design/brand/mark/grid.json)
 * drawn as SVG, with a CSS loop. The two-leaf sprout grows, unfurls and sways out of the
 * head and the eyes blink. Under `prefers-reduced-motion` it is a static calm pose.
 */
export function SpiritLoader({
  label = "StackSpirit is thinking",
  size = 28,
  showLabel = false,
}: {
  label?: string;
  size?: number;
  showLabel?: boolean;
}) {
  const height = Math.round((size * SPRITE_H) / SPRITE_W);
  return (
    <span className="nx-loader" role="status" data-testid="spirit-loader">
      <svg
        className="nx-loader__svg"
        viewBox={`0 0 ${SPRITE_W} ${SPRITE_H}`}
        width={size}
        height={height}
        shapeRendering="crispEdges"
        aria-hidden="true"
        focusable="false"
      >
        <g className="nx-loader__sprout">
          <Layer name="stem" />
          <g className="nx-loader__leaf nx-loader__leaf--left">
            <Layer name="leafLeft" />
          </g>
          <g className="nx-loader__leaf nx-loader__leaf--right">
            <Layer name="leafRight" />
          </g>
        </g>
        <Layer name="body" />
        {SPRITE_EYES.map((box, index) => (
          <g
            key={index}
            className="nx-loader__eye"
            style={{
              transformOrigin: `${(box.left + box.right) / 2}px ${(box.top + box.bottom) / 2}px`,
            }}
          >
            <Layer name={`eye${index}`} />
          </g>
        ))}
      </svg>
      {showLabel ? (
        <span className="nx-loader__label" aria-hidden="true">
          {label}
        </span>
      ) : null}
      <span className="nx-sr-only">{label}</span>
    </span>
  );
}

/**
 * The inline chat-turn indicator: three Meadow green dots orbiting in a 16px mark next to
 * shimmering elapsed text (`Thinking…` then `Thinking for 3s ···`). It fades in after a
 * short delay so an instant reply never flashes it, and is static under
 * `prefers-reduced-motion`. The text is visible and is the accessible name.
 */
export function TurnLoader({ startedAt }: { startedAt?: number }) {
  const [origin] = useState(() => startedAt ?? Date.now());
  const [seconds, setSeconds] = useState(() =>
    Math.max(0, Math.floor((Date.now() - origin) / 1000)),
  );
  useEffect(() => {
    const timer = setInterval(
      () => setSeconds(Math.max(0, Math.floor((Date.now() - origin) / 1000))),
      1000,
    );
    return () => clearInterval(timer);
  }, [origin]);
  const text = seconds < 1 ? "Thinking…" : `Thinking for ${seconds}s`;
  return (
    <div className="nx-turn-loader" role="status" data-testid="turn-loader">
      <svg
        className="nx-turn-loader__dots"
        width="16"
        height="16"
        viewBox="0 0 16 16"
        aria-hidden="true"
        focusable="false"
        data-testid="turn-loader-dots"
      >
        <g className="nx-turn-loader__orbit">
          <circle cx="8" cy="2.5" r="2" fill="#2f8547" />
          <circle cx="12.76" cy="10.75" r="2" fill="#52b86d" />
          <circle cx="3.24" cy="10.75" r="2" fill="#78c462" />
        </g>
      </svg>
      <span className="nx-turn-loader__text" data-testid="turn-loader-text">
        {text}
        {seconds >= 1 ? <span aria-hidden="true"> ···</span> : null}
      </span>
    </div>
  );
}
