#!/usr/bin/env bash
# Poll the submitted change set. Validation runs from minutes to hours;
# the AMI security scan is the slow part.
set -euo pipefail
export AWS_CA_BUNDLE=${AWS_CA_BUNDLE:-/etc/ssl/cert.pem}
export AWS_PAGER=''
cd "$(dirname "$0")"
ID=$(python3 -c 'import json;print(json.load(open("submit-result.json"))["ChangeSetId"])')
aws marketplace-catalog describe-change-set \
  --profile marketplace --region us-east-1 \
  --catalog AWSMarketplace --change-set-id "$ID" \
  --query '{Status:Status,Errors:FailureDescription,Changes:ChangeSet[].{Type:ChangeType,Errors:ErrorDetailList}}' \
  --output json
