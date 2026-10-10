<div align="center">
  <a href="README.md"><b>OnyxDiskInfo</b></a>
</div>

# Terms and Conditions

These terms govern your use of OnyxDiskInfo, an open source tool that reads
the health of a disk, and tests, formats and erases a disk (the "Software").
By using, building, running, or contributing to the Software, you agree to
these terms.
If you do not agree, do not use the Software.

## 1. Licence

The Software is licensed under the MIT License, in the [LICENSE](LICENSE) file.
That licence is the authoritative statement of your rights to use, copy,
modify, and distribute the Software.
If anything here conflicts with the LICENSE file, the LICENSE file governs.

## 2. No warranty

The Software is provided "as is", without warranty of any kind, express or
implied.
This includes the warranties of merchantability, fitness for a particular
purpose, and noninfringement.
You use the Software at your own risk.

## 3. Limitation of liability

To the maximum extent that the law permits, the authors and copyright holders
are not liable for any claim, damages, or other liability that arises from the
use of the Software.
This applies whether the claim is in contract, in tort, or otherwise.

## 4. The Software can destroy data

**Read this section before you format, erase, or write test a disk.**

A format, an erase, and a write test write directly to a block device.
They erase everything on the target drive.
The erased data does not go to a recycle bin, and no undo exists.
A secure erase makes the data unrecoverable on purpose.

The Software asks you to confirm the target.
That confirmation is the last check, and it is yours to make.
The Software cannot know which drive holds the only copy of your photographs.

You are responsible for the target that you select.
Keep a backup of anything that you value before you start.

## 5. A health value is not a promise

The Software shows what a drive reports, and what the Software measures.
A drive can fail with a good health value.
A drive can also report a bad value and work for years.
Some values come from the tests of the Software and not from the drive, and
the Software shows a confidence level for each one.

Do not use a health value as the only reason to keep data on a drive.
Keep a backup.

## 6. Contributions

If you contribute to the Software, you agree that your contribution carries the
same MIT Licence as the rest of the project.
See [CONTRIBUTING.md](CONTRIBUTING.md) for how to take part.

## 7. Third-party components

The Software uses open source libraries.
Each of those carries its own licence, and that licence governs that component.
The Software can run `smartctl` from smartmontools, and the format tools of
your operating system, if they are on your computer.
The Software does not include or distribute them.
They come from their own publishers, under their own terms.

## 8. Project name

"OnyxDiskInfo", "Onyx", and the `onyx` command name identify this project.
You may refer to the project by name.
Please do not use the name in a way that suggests endorsement of, or connection
with, a modified or unofficial version, without permission.
The project has no connection to CrystalDiskInfo or to its author.

## 9. Changes to these terms

These terms may change as the project grows.
The version in the default branch of the repository is the current one.
If you continue to use the Software after a change, you accept the updated
terms.
