```
encrypt_1024B           time:   [16.339 µs 16.436 µs 16.548 µs]
                        change: [-2.0278% -1.6180% -1.1825%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 10 outliers among 100 measurements (10.00%)
  1 (1.00%) low mild
  2 (2.00%) high mild
  7 (7.00%) high severe

decrypt_1024B           time:   [4.5905 µs 4.5963 µs 4.6028 µs]
                        change: [-2.6843% -2.1965% -1.7929%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 1 outliers among 100 measurements (1.00%)
  1 (1.00%) low mild

encrypt_1048576B        time:   [16.055 ms 16.079 ms 16.108 ms]
                        change: [-1.7809% -1.6178% -1.4274%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 7 outliers among 100 measurements (7.00%)
  6 (6.00%) high mild
  1 (1.00%) high severe

decrypt_1048576B        time:   [4.1162 ms 4.1239 ms 4.1318 ms]
                        change: [-1.0261% -0.8151% -0.6079%] (p = 0.00 < 0.05)
                        Change within noise threshold.

encrypt_file_1.0KiB     time:   [150.00 µs 150.38 µs 150.79 µs]
                        change: [+77728% +78154% +78620%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 5 outliers among 100 measurements (5.00%)
  3 (3.00%) high mild
  2 (2.00%) high severe

encrypt_file_1.0MiB     time:   [3.9655 ms 3.9763 ms 3.9900 ms]
                        change: [+2064745% +2071335% +2078942%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 7 outliers among 100 measurements (7.00%)
  2 (2.00%) high mild
  5 (5.00%) high severe

Benchmarking encrypt_file_100.0MiB: Warming up for 3.0000 s
Warning: Unable to complete 100 samples in 5.0s. You may wish to increase target time to 39.4s, or reduce sample count to 10.
encrypt_file_100.0MiB   time:   [391.77 ms 392.58 ms 393.60 ms]
                        change: [+201954376% +202968329% +203917717%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 16 outliers among 100 measurements (16.00%)
  3 (3.00%) high mild
  13 (13.00%) high severe

Benchmarking encrypt_file_1.0GiB: Warming up for 3.0000 s
Warning: Unable to complete 100 samples in 5.0s. You may wish to increase target time to 407.1s, or reduce sample count to 10.
encrypt_file_1.0GiB     time:   [3.9879 s 3.9914 s 3.9952 s]
                        change: [+2077941170% +2082070793% +2086105171%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 3 outliers among 100 measurements (3.00%)
  3 (3.00%) high mild
```