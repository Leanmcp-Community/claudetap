# AMI build result — 2026-09-08

- AWS profile: `marketplace`
- Account: `187692046617`
- Region: `us-west-2`
- Private AMI: `ami-00a251a4a1b5414ed`
- EBS snapshot: `snap-0785e27a3d841cb66`
- Base AMI: `ami-04678417fc39d7171` (Canonical Ubuntu 24.04, x86_64)
- Root disk: 16 GiB gp3; publisher image snapshot is unencrypted.
- IMDS: v2 required by AMI configuration.
- Runtime: native Python virtual environment and systemd; Caddy for optional HTTPS.

Validation on a fresh t3.small instance passed: boot and SSH with a new launch
key, initially empty database, first-boot credential creation, UI, authenticated
policy fetch, upload, duplicate retry, file retrieval, and health after service
restart. Synthetic data was written only to the disposable test instance, not
the AMI. No public Marketplace listing or offer was created.

Not validated: public DNS/HTTPS issuance, AWS Marketplace security scan, formal
vulnerability audit, backup restore, upgrade migration, sustained load or HA.
The image is a tested packaging candidate, not an AWS-approved listing.

Verify the image:

```bash
aws ec2 describe-images \
  --profile marketplace \
  --region us-west-2 \
  --image-ids ami-00a251a4a1b5414ed
```

If the local CLI needs the system CA bundle, prefix the command with
`AWS_CA_BUNDLE=/etc/ssl/cert.pem`. Do not disable certificate verification.

Buyer instructions: [AMI_USAGE.md](AMI_USAGE.md).

Temporary builder/test instances, SSH key pair and security group were removed
after successful validation. The retained AMI snapshot continues to incur storage
charges; no test EC2 instance was left running.
