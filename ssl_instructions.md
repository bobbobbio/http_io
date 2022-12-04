# How to generate the test certificates

See `man openssl ca` for more information

To generate a certificate authority certificate / private key

    openssl req -x509 -newkey rsa:4096 -keyout test_ca_key.pem -out test_ca.pem -sha256 -nodes -extensions v3_ca -days 365000

Configure certificate authority via openssl.cnf file

have a directory structure like this

```
demoCA/
├── cacert.pem
├── index.txt
├── newcerts
│   ├── 01FBEAAD0277F55E582FE10A0664841BE972ACC3.pem
│   └── 6EBCAA13B6FEDFB1A3D0EF4CAFCC98D145E732.pem
└── private
    └── cakey.pem

```

Generate a private key / certificate to be signed for "localhost"
This certificate will be replaced with the signed one later

    openssl req -x509 -newkey rsa:4096 -keyout test_key.pem -out test_cert.pem -sha256 -nodes -subj '/CN=localhost'

Generate certificate signing request
When prompted for "Common Name" enter "localhost"

    openssl req -new -sha256 -key test_key.pem -out test_cert.csr.pem -addext "subjectAltName = DNS:localhost"

Sign the request

    openssl ca -in test_cert.csr.pem -out test_cert.pem -extensions v3_req -days 365000

If you need to revoke a certificate

    openssl ca -revoke demoCA/newcerts/27CA09DB1FBC9AC4BA6A8697EB68C026CB8C7558.pem
