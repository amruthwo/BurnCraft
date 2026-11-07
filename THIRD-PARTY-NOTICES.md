# Third-party notices

BurnCraft is licensed under the GNU GPL v3 or later (see [LICENSE](LICENSE)). Parts of it are based on the
open-source projects below, whose licences allow this. Their notices are kept here as those licences require.

## K40 Whisperer

K40 / M2 Nano support (`src/k40.rs`) is ported from K40 Whisperer 0.68 by Scorch, <https://www.scorchworks.com/>.

- `egv.py` and `nano_library.py`: Copyright (C) 2017-2023 Scorch. GNU General Public License, version 2 or (at
  your option) any later version. BurnCraft uses it under version 3.
- `LaserSpeed.py`: MIT License (same text as below).

## MeerK40t

The EZCad2 (.ezd) import (`src/ezd.rs`) follows the file layout worked out in MeerK40t,
<https://github.com/meerk40t/meerk40t>, and galvo support (`src/galvo.rs`) follows MeerK40t's galvo driver
(`meerk40t/balormk`): its USB protocol, command numbers, start-up sequence, unit conversions and defaults.

```text
MIT License

Copyright (c) 2021 meerk40t

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```
